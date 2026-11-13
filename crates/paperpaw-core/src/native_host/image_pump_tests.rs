//! The picture pump: commands apply after the callback, one request decodes
//! a turn, answers reach the plugin that asked (and no other), and the cache
//! folder is swept once before the first decode.
#![allow(clippy::unwrap_used, reason = "fixture errors fail tests")]
extern crate std;
use super::*;
use crate::native_host::{
    native_instances::{NativeInstance, NativeLease},
    native_screen::Dispatch,
};
use alloc::rc::Rc;
use paperpaw_native::host_glue::{ERR_INVALID, OK};
use paperpaw_platform::Instant;
use paperpaw_sdk::{Id, MemoryStorage, image::PngWriter};

/// What a plugin was told: `(cookie, status, width, height)`.
type Told = (u32, u32, u32, u32);

struct Probe {
    told: Rc<RefCell<Vec<Told>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
    subscribed: bool,
    running: Cell<bool>,
}

impl NativeInstance for Probe {
    fn running(&self) -> bool {
        self.running.get()
    }
    fn stop(&self) {
        self.running.set(false);
    }
    fn subscribed(&self, tag: u32) -> bool {
        self.subscribed && tag == wire::EVENT_IMAGE_READY_TAG
    }
    fn service_event(&self, bytes: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        match wire::records_decode_single(bytes)? {
            wire::Record::EventImageReady(e) => {
                self.told
                    .borrow_mut()
                    .push((e.cookie, e.status, e.width, e.height));
                self.answer.get()
            }
            _ => Err(ERR_INVALID),
        }
    }
}

impl Dispatch for Probe {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_INVALID)
    }
    fn dropped(&self, _: u32, _: i32) {}
}

struct World {
    kernel: crate::Kernel,
    owner: u64,
    told: Rc<RefCell<Vec<Told>>>,
    answer: Rc<Cell<Result<i32, i32>>>,
}

fn world(subscribed: bool) -> World {
    let storage = MemoryStorage::new();
    // The plugin's own folder (root 0) holds the pictures.
    let mut writer = PngWriter::new(&storage, ".paperpaw/plugins/a/p.png", 40, 20).unwrap();
    for _ in 0..20 {
        writer
            .row(&storage, ".paperpaw/plugins/a/p.png", &[90_u8; 40])
            .unwrap();
    }
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(storage)
        .build()
        .unwrap();
    let told = Rc::new(RefCell::new(Vec::new()));
    let answer = Rc::new(Cell::new(Ok(OK)));
    let lease = NativeLease::try_new(Probe {
        told: Rc::clone(&told),
        answer: Rc::clone(&answer),
        subscribed,
        running: Cell::new(true),
    })
    .unwrap();
    let owner = kernel
        .shared
        .native_instances
        .test_insert(Id::try_parse("a").unwrap(), Some(lease), true)
        .unwrap();
    World {
        kernel,
        owner,
        told,
        answer,
    }
}

fn prepare(root: u32, path: &str, cookie: u32) -> ImageCommand {
    ImageCommand::Prepare {
        root,
        path: String::from(path),
        side: 0,
        cookie,
        colour: false,
        recipe: Default::default(),
    }
}

impl World {
    fn apply(&mut self, commands: Vec<ImageCommand>) {
        self.kernel.native_images_apply(self.owner, commands);
    }
    fn told(&self) -> Vec<Told> {
        self.told.borrow_mut().drain(..).collect()
    }
    fn pending(&self) -> usize {
        self.kernel
            .shared
            .native_instances
            .image_pump
            .pending
            .borrow()
            .len()
    }
}

#[test]
fn one_request_decodes_a_turn_and_its_plugin_is_told() {
    let mut w = world(true);
    w.apply(std::vec![
        prepare(0, "p.png", 1),
        prepare(0, "p.png", 2),
        prepare(0, "missing.png", 3),
    ]);
    // Nothing happens inside the apply: the decode is the pump's, a turn each.
    assert!(w.told().is_empty());
    assert!(w.kernel.native_image_pump(), "two requests are left");
    let first = w.told();
    assert_eq!(first.len(), 1);
    assert_eq!((first[0].0, first[0].1), (1, 0));
    assert!(first[0].2 > 0 && first[0].3 > 0);
    assert!(w.kernel.native_image_pump());
    assert_eq!(
        w.told().iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(),
        [(2, 0)]
    );
    // The third source is not there: a damaged or card answer, never a hang.
    assert!(!w.kernel.native_image_pump());
    let last = w.told();
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].0, 3);
    assert_ne!(last[0].1, 0);
    assert!(!w.kernel.native_image_pump(), "nothing left");
}

#[test]
fn root_one_never_reaches_the_plugins_folder_or_the_hosts() {
    let mut w = world(true);
    w.apply(std::vec![
        prepare(1, "plugins/a/p.png", 1),
        prepare(1, ".PaperPaw/plugins/a/p.png", 2),
        prepare(0, ".received/x.png", 3),
        prepare(2, "p.png", 4),
    ]);
    while w.kernel.native_image_pump() {}
    let mut told = w.told();
    told.sort_unstable();
    // Each is answered with the card status (4), none was decoded; the
    // folder of a plugin that is no running data plugin is not there (6).
    assert_eq!(
        told.iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(),
        [(1, 6), (2, 4), (3, 4), (4, 4)]
    );
}

#[test]
fn root_one_reaches_a_running_data_plugins_files_only() {
    use crate::native_host::{
        data_plugin_folders::DataPluginFolders, host_services::HostServices,
        storage_files::StorageFiles,
    };
    let hosts = HostServices::new();
    let folders = Rc::new(DataPluginFolders::default());
    folders.replace(["owls"].into_iter(), 1).unwrap();
    let storage: Rc<dyn paperpaw_platform::Storage> = Rc::new(MemoryStorage::new());
    hosts
        .wire_files(Rc::new(
            StorageFiles::new(storage, Rc::default(), Rc::default())
                .with_data_plugins(folders.clone()),
        ))
        .unwrap();
    let roots = Roots {
        plugin: "sleep",
        hosts: &hosts,
    };
    let resolve = |path: &str| roots.resolve(1, 1, path);
    assert_eq!(
        resolve("plugins/owls/owl.png").as_deref(),
        Ok("plugins/owls/owl.png")
    );
    assert_eq!(
        resolve("PLUGINS/OWLS/a.png").as_deref(),
        Ok("PLUGINS/OWLS/a.png")
    );
    // A code plugin's folder, a prefix of the name and an alias are not there.
    for gone in [
        "plugins/weather/a.png",
        "plugins/owl/a.png",
        "plugins/OWLS~1/a.png",
    ] {
        assert_eq!(resolve(gone), Err(ERR_NOT_FOUND), "{gone}");
    }
    // Its code and the host's files are not there either (`files.md` 1.1).
    for hidden in ["plugins/owls/code.ppn", "plugins/OWLS/.install-marks"] {
        assert_eq!(resolve(hidden), Err(ERR_NOT_FOUND), "{hidden}");
    }
    // The kernel's own folders stay refused.
    assert_eq!(resolve(".paperpaw/cache/a.png"), Err(ERR_DENIED));
    // A data plugin that stops running is not there any more.
    folders.replace(core::iter::empty(), 2).unwrap();
    assert_eq!(resolve("plugins/owls/owl.png"), Err(ERR_NOT_FOUND));
}

#[test]
fn a_full_table_keeps_the_command_and_applies_it_at_its_own_time() {
    let mut w = world(true);
    w.kernel.now = Instant(1_000);
    // Four wait at most; the fifth is kept, in order.
    w.apply((1..=5).map(|cookie| prepare(0, "p.png", cookie)).collect());
    assert_eq!(w.pending(), 1);
    let mut cookies = Vec::new();
    // The four decode, a turn each. The held command does not ask for another
    // pass: when the decoding is over the pump is quiet, and its time is the
    // 2 s back-off.
    while w.kernel.native_image_pump() {
        cookies.extend(w.told().iter().map(|t| t.0));
    }
    cookies.extend(w.told().iter().map(|t| t.0));
    assert_eq!(cookies, [1, 2, 3, 4]);
    assert_eq!(w.pending(), 1, "not applied before its time");
    assert_eq!(w.kernel.service_due(), Some(Instant(3_000)));
    w.kernel.now = Instant(3_000);
    while w.kernel.native_image_pump() {}
    assert_eq!(w.pending(), 0);
    assert_eq!(w.told().iter().map(|t| t.0).collect::<Vec<_>>(), [5]);
    assert_eq!(w.kernel.service_due(), None);
}

#[test]
fn a_command_that_finds_no_room_after_its_tries_fails_to_its_plugin() {
    let mut w = world(true);
    w.kernel.now = Instant(1_000);
    w.apply((1..=5).map(|cookie| prepare(0, "p.png", cookie)).collect());
    // Nothing decodes meanwhile, so the table stays full: three tries, 2 s
    // apart, then the plugin is told (status 5, no memory now).
    for at in [3_000, 5_000, 7_000] {
        assert_eq!(w.pending(), 1);
        w.kernel.now = Instant(at);
        w.kernel.image_retry();
    }
    assert_eq!(w.pending(), 0);
    while w.kernel.native_image_pump() {}
    let told = w.told();
    assert!(told.iter().any(|t| (t.0, t.1) == (5, 5)), "{told:?}");
}

#[test]
fn what_cannot_be_kept_fails_to_its_plugin_and_nothing_grows() {
    let mut w = world(true);
    w.apply((0..40).map(|cookie| prepare(0, "p.png", cookie)).collect());
    // 4 queued, 16 kept (the bound); of the other 20 the failures have room
    // for eight, and the oldest give way: the last eight are told.
    assert_eq!(w.pending(), PENDING_MAX);
    w.kernel.native_image_pump();
    let told = w.told();
    let mut failed: Vec<_> = told.iter().filter(|t| t.1 == 5).map(|t| t.0).collect();
    failed.sort_unstable();
    assert_eq!(failed, (32..40).collect::<Vec<_>>());
}

#[test]
fn a_plugin_that_did_not_subscribe_loses_the_answer_without_a_call() {
    let mut w = world(false);
    w.apply(std::vec![prepare(0, "p.png", 1)]);
    while w.kernel.native_image_pump() {}
    assert!(w.told().is_empty());
    // And nothing stays held for it.
    assert!(
        w.kernel
            .shared
            .native_instances
            .image_pump
            .held
            .borrow()
            .is_none()
    );
}

#[test]
fn a_failing_callback_ends_the_answer_but_busy_keeps_it() {
    let mut w = world(true);
    w.apply(std::vec![prepare(0, "p.png", 1)]);
    w.answer.set(Err(ERR_BUSY));
    assert!(w.kernel.native_image_pump(), "kept for the next pass");
    assert_eq!(w.told().len(), 1);
    // Told again; this time it fails: the answer ends with that call.
    w.answer.set(Err(ERR_INVALID));
    w.kernel.native_image_pump();
    assert_eq!(w.told().len(), 1);
    w.answer.set(Ok(OK));
    assert!(!w.kernel.native_image_pump());
    assert!(w.told().is_empty(), "not told a third time");
}

#[test]
fn a_stopped_plugin_gets_nothing_and_its_commands_are_dropped() {
    let mut w = world(true);
    w.apply(std::vec![prepare(0, "p.png", 1)]);
    w.kernel.shared.native_instances.stop(w.owner).unwrap();
    while w.kernel.native_image_pump() {}
    assert!(w.told().is_empty());
    // A plugin that is gone has no pictures to ask for.
    w.apply(std::vec![prepare(0, "p.png", 2)]);
    assert_eq!(w.pending(), 0);
}

#[test]
fn the_cache_folder_is_swept_once_before_the_first_decode() {
    let mut w = world(true);
    let storage = w.kernel.storage();
    storage.create_dir(".paperpaw").unwrap();
    storage.create_dir(".paperpaw/cache").unwrap();
    storage.create_dir(".paperpaw/cache/images").unwrap();
    storage
        .write(".paperpaw/cache/images/1-1.img", b"left by a power cut")
        .unwrap();
    // A file that is not this host's stays.
    storage
        .write(".paperpaw/cache/images/notes.txt", b"mine")
        .unwrap();
    w.apply(std::vec![prepare(0, "p.png", 1)]);
    while w.kernel.native_image_pump() {}
    let storage = w.kernel.storage();
    assert!(!storage.exists(".paperpaw/cache/images/1-1.img"));
    assert!(storage.exists(".paperpaw/cache/images/notes.txt"));
    assert_eq!(w.told().len(), 1, "the request was decoded after the sweep");
}

#[test]
fn a_lasting_refusal_at_apply_is_told_as_unsupported() {
    let mut w = world(true);
    // The submit said yes; the table finds the source too long for good.
    let long = "x".repeat(2_000);
    w.apply(std::vec![prepare(0, &long, 9)]);
    assert_eq!(w.pending(), 0);
    while w.kernel.native_image_pump() {}
    assert_eq!(w.told(), [(9, 1, 0, 0)]);
}

#[test]
fn when_every_failure_place_is_taken_the_oldest_gives_way() {
    let mut w = world(true);
    let long = "x".repeat(2_000);
    // Ten failures, eight places: the first two give way to the last two.
    w.apply((0..10).map(|cookie| prepare(0, &long, cookie)).collect());
    assert_eq!(w.kernel.image_held(w.owner), 8);
    while w.kernel.native_image_pump() {}
    let mut cookies: Vec<u32> = w.told().iter().map(|t| t.0).collect();
    cookies.sort_unstable();
    assert_eq!(cookies, (2..10).collect::<Vec<_>>());
    assert_eq!(w.kernel.image_held(w.owner), 0);
}

#[test]
fn requests_held_outside_the_table_count_against_the_plugins_room() {
    use crate::native_host::{
        commands::{CommandStage, WireCommand, commands_decode, commands_encode},
        image_service::commands::ImageAdmit,
    };
    let owner = Id::new_static("a");
    let settings = paperpaw_sdk::Settings::new();
    let mut stage = CommandStage::new(&owner, &settings, None, 8, 4096);
    stage.set_images_held(3);
    stage.admit_images(ImageAdmit {
        storage_root: false,
        room: 4,
    });
    let command = WireCommand::ImagePrepare {
        root: 0,
        path: "p.png",
        side: 0,
        cookie: 1,
        flags: 0,
        tone: &[],
    };
    let mut bytes = [0; 256];
    let two = commands_encode(&[command, command], &mut bytes).unwrap();
    // Room 4 less 3 held: one more only.
    assert_eq!(
        stage.stage(&commands_decode(&bytes[..two]).unwrap()),
        paperpaw_native::host_glue::ERR_FULL
    );
    let one = commands_encode(&[command], &mut bytes).unwrap();
    assert_eq!(stage.stage(&commands_decode(&bytes[..one]).unwrap()), OK);
}
