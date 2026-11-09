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
    // Each is answered with the card status (4), none was decoded.
    assert_eq!(
        told.iter().map(|t| (t.0, t.1)).collect::<Vec<_>>(),
        [(1, 4), (2, 4), (3, 4), (4, 4)]
    );
}

#[test]
fn a_full_table_keeps_the_command_and_applies_it_at_a_later_pass() {
    let mut w = world(true);
    // Four wait at most; the fifth is kept, in order.
    w.apply((1..=5).map(|cookie| prepare(0, "p.png", cookie)).collect());
    assert_eq!(w.pending(), 1);
    let mut cookies = Vec::new();
    let mut passes = 0;
    while w.kernel.native_image_pump() && passes < 20 {
        cookies.extend(w.told().iter().map(|t| t.0));
        passes += 1;
    }
    cookies.extend(w.told().iter().map(|t| t.0));
    assert_eq!(cookies, [1, 2, 3, 4, 5]);
    assert_eq!(w.pending(), 0);
}

#[test]
fn what_cannot_be_kept_is_dropped_not_grown() {
    let mut w = world(true);
    w.apply((0..40).map(|cookie| prepare(0, "p.png", cookie)).collect());
    // 4 queued, then the rest kept up to the bound, the remainder dropped.
    assert_eq!(w.pending(), PENDING_MAX);
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
