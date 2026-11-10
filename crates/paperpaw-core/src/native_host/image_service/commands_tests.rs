#![allow(clippy::unwrap_used, reason = "fixture errors fail the test")]
//! Admission and application of the two image commands.
extern crate std;
use super::*;
use paperpaw_native::host_glue::ERR_NOT_FOUND;
use paperpaw_platform::Storage;
use paperpaw_sdk::MemoryStorage;
use paperpaw_sdk::image::PngWriter;

const A: u64 = 1;

fn prepare_cmd(root: u32, path: &str, flags: u32) -> WireCommand<'_> {
    WireCommand::ImagePrepare {
        root,
        path,
        side: 16,
        cookie: 5,
        flags,
    }
}

fn admit(storage_root: bool, room: usize) -> ImageAdmit {
    ImageAdmit { storage_root, room }
}

#[test]
fn admission_checks_shape_roots_flags_and_room_without_side_effects() {
    let mut a = admit(false, 2);
    assert_eq!(a.admit(&prepare_cmd(0, "p/a.png", 0)), Some(Ok(())));
    assert_eq!(a.admit(&prepare_cmd(0, "p/a.png", 1)), Some(Ok(())));
    assert_eq!(a.admit(&prepare_cmd(0, "b.png", 0)), Some(Err(ERR_FULL)));
    let mut a = admit(false, 9);
    assert_eq!(a.admit(&prepare_cmd(1, "a.png", 0)), Some(Err(ERR_DENIED)));
    // Root 2, the plugin's install folder: read only, no feature.
    assert_eq!(a.admit(&prepare_cmd(2, "a.png", 0)), Some(Ok(())));
    assert_eq!(
        a.admit(&prepare_cmd(3, "a.png", 0)),
        Some(Err(ERR_UNSUPPORTED))
    );
    assert_eq!(a.room, 8, "root 2 took one room");
    assert_eq!(
        a.admit(&prepare_cmd(0, "a.png", 2)),
        Some(Err(ERR_UNSUPPORTED))
    );
    for bad in ["", "/a", "../a", "a/../b", "a//b", "a\\b", "a\0b"] {
        assert_eq!(
            a.admit(&prepare_cmd(0, bad, 0)),
            Some(Err(ERR_INVALID)),
            "{bad:?}"
        );
    }
    // As `core.files` checks a path: its grammar, its sizes, and root 1 never
    // reaches the install folders or the kernel's.
    for bad in ["a:b", "x.", "CON", "a\u{1}b"] {
        assert_eq!(
            a.admit(&prepare_cmd(0, bad, 0)),
            Some(Err(ERR_INVALID)),
            "{bad:?}"
        );
    }
    assert_eq!(
        a.admit(&prepare_cmd(0, &"p".repeat(600), 0)),
        Some(Err(paperpaw_native::host_glue::ERR_NO_SPACE))
    );
    assert_eq!(a.room, 8, "a refused command took no room");
    let mut with_card = admit(true, 4);
    for bad in [
        "plugins/x/a.png",
        "Plugins/a.png",
        ".paperpaw/cache/a.png",
        // The kernel's settings file, in any case and as the driver folds.
        "paperpaw.ini",
        "PAPERPAW.INI",
        "paperpaw.ını",
    ] {
        assert_eq!(
            with_card.admit(&prepare_cmd(1, bad, 0)),
            Some(Err(ERR_INVALID)),
            "{bad:?}"
        );
    }
    assert_eq!(
        with_card.admit(&prepare_cmd(1, "Pics/a.png", 0)),
        Some(Ok(()))
    );
    assert_eq!(
        with_card.admit(&prepare_cmd(1, "Pics/paperpaw.ini", 0)),
        Some(Ok(())),
        "below a folder it is an ordinary name"
    );
    assert_eq!(
        admit(true, 1).admit(&prepare_cmd(1, "a.png", 0)),
        Some(Ok(()))
    );
    assert_eq!(
        a.admit(&WireCommand::ImageRelease { image: 3 }),
        Some(Ok(()))
    );
    assert_eq!(a.admit(&WireCommand::Redraw { mode: 0 }), None);
    assert!(owned(&WireCommand::Redraw { mode: 0 }).is_none());
}

struct Roots;
impl ImageRoots for Roots {
    fn resolve(&self, _: u64, root: u32, path: &str) -> Result<String, i32> {
        match root {
            0 => {
                // As the shell's does: room taken fallibly.
                let mut source = String::new();
                source
                    .try_reserve_exact(5 + path.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                source.push_str("data/");
                source.push_str(path);
                Ok(source)
            }
            // The install folder of the plugin "demo", as `core.files` has it.
            2 => bundle_source("demo", false, path),
            _ => Err(ERR_NOT_FOUND),
        }
    }
}

fn png(storage: &MemoryStorage, path: &str, w: u32, h: u32) {
    let mut writer = PngWriter::new(storage, path, w, h).unwrap();
    let row = std::vec![90_u8; w as usize];
    for _ in 0..h {
        writer.row(storage, path, &row).unwrap();
    }
}

fn told(t: &ImageTable) -> Option<prepare::Ready> {
    let mut got = None;
    t.take_ready(&mut |r| {
        got = Some(r.clone());
        Ok(())
    })
    .unwrap();
    got
}

#[test]
fn an_admitted_prepare_is_queued_decoded_and_answered_and_a_release_ends_it() {
    let t = ImageTable::new();
    let storage = MemoryStorage::new();
    png(&storage, "data/a.png", 40, 20);
    let cmd = owned(&prepare_cmd(0, "a.png", 0)).unwrap().unwrap();
    assert_eq!(t.prepare_room(A), 4);
    t.apply(A, &cmd, &Roots).unwrap();
    assert_eq!(t.prepare_room(A), 3);
    assert!(
        t.prepare_step(
            &storage,
            prepare::Env {
                screen: 480,
                colour: false,
                max_side: 4096,
                png_row_max: usize::MAX,
            },
        )
        .unwrap()
    );
    let ready = told(&t).unwrap();
    assert_eq!(
        (ready.owner, ready.cookie, ready.status),
        (A, 5, prepare::READY)
    );
    assert_eq!(
        t.prepare_room(A),
        4,
        "the request is told: 4 may wait again"
    );
    t.apply(A, &ImageCommand::Release { image: ready.image }, &Roots)
        .unwrap();
    assert_eq!(t.prepare_room(A), 4);
    // Releasing what is not there is nothing.
    t.apply(A, &ImageCommand::Release { image: 99 }, &Roots)
        .unwrap();
}

#[test]
fn a_source_that_does_not_resolve_is_answered_not_lost() {
    let t = ImageTable::new();
    let cmd = owned(&prepare_cmd(1, "a.png", 0)).unwrap().unwrap();
    t.apply(A, &cmd, &Roots).unwrap();
    let ready = told(&t).unwrap();
    assert_eq!(
        (ready.status, ready.image, ready.cookie),
        (prepare::NOT_FOUND, 0, 5)
    );
    // A request over the room is the caller's ERR_FULL.
    for _ in 0..4 {
        let cmd = owned(&prepare_cmd(0, "x.png", 0)).unwrap().unwrap();
        t.apply(A, &cmd, &Roots).unwrap();
    }
    let cmd = owned(&prepare_cmd(0, "x.png", 0)).unwrap().unwrap();
    assert_eq!(t.apply(A, &cmd, &Roots), Err(ERR_FULL));
}

#[test]
fn an_answer_stays_until_it_is_taken_and_goes_with_its_plugin() {
    let t = ImageTable::new();
    t.refuse(A, 1, prepare::CARD).unwrap();
    assert!(t.prepare_pending());
    assert_eq!(
        t.take_ready(&mut |_| Err(ERR_NO_MEMORY)),
        Err(ERR_NO_MEMORY)
    );
    assert!(t.prepare_pending(), "a refused delivery keeps it");
    t.owner_stopped(A).unwrap();
    assert_eq!(t.take_ready(&mut |_| Ok(())), Ok(false));
}

#[test]
fn the_stage_admits_image_commands_atomically_and_hands_them_over() {
    use crate::native_host::commands::{CommandScope, CommandStage, commands_encode};
    use paperpaw_sdk::Id;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_sdk::Instant(0));
    let owner = Id::new_static("probe");
    let submit = |stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]| {
        let scope = core::pin::pin!(CommandScope::new());
        let mut bytes = [0; 512];
        let len = commands_encode(commands, &mut bytes).unwrap();
        scope
            .as_ref()
            .scope(stage, || {
                // SAFETY: the encoded input stays readable until the call ends.
                unsafe { scope.submit(bytes.as_ptr(), len) }
            })
            .unwrap()
    };
    // Not admitted by the host: unsupported, as before.
    let mut plain = CommandStage::new(&owner, env.settings(), None, 8, 4096);
    assert_eq!(
        submit(&mut plain, &[prepare_cmd(0, "a.png", 0)]),
        ERR_UNSUPPORTED
    );
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 8, 4096).with_images(admit(false, 2));
    // One bad command refuses the whole batch and takes no room.
    assert_eq!(
        submit(
            &mut stage,
            &[prepare_cmd(0, "a.png", 0), prepare_cmd(1, "b.png", 0)]
        ),
        ERR_DENIED
    );
    assert!(stage.take_image_commands().is_empty());
    assert_eq!(
        submit(
            &mut stage,
            &[
                prepare_cmd(0, "a.png", 0),
                WireCommand::ImageRelease { image: 4 }
            ]
        ),
        0
    );
    // The room was one of two; a second request fits, a third is ERR_FULL.
    assert_eq!(submit(&mut stage, &[prepare_cmd(0, "b.png", 1)]), 0);
    assert_eq!(submit(&mut stage, &[prepare_cmd(0, "c.png", 0)]), ERR_FULL);
    let taken = stage.take_image_commands();
    assert_eq!(taken.len(), 3);
    assert!(matches!(taken[1], ImageCommand::Release { image: 4 }));
    assert!(matches!(
        taken[2],
        ImageCommand::Prepare { colour: true, .. }
    ));
}

#[test]
fn image_paths_count_against_the_callbacks_byte_budget() {
    use crate::native_host::commands::{CommandScope, CommandStage, commands_encode};
    use paperpaw_native::host_glue::ERR_NO_SPACE;
    use paperpaw_sdk::Id;
    let kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .build()
        .unwrap();
    let env = kernel.shared.env(paperpaw_sdk::Instant(0));
    let owner = Id::new_static("probe");
    let submit = |stage: &mut CommandStage<'_>, commands: &[WireCommand<'_>]| {
        let scope = core::pin::pin!(CommandScope::new());
        let mut bytes = [0; 512];
        let len = commands_encode(commands, &mut bytes).unwrap();
        scope
            .as_ref()
            .scope(stage, || {
                // SAFETY: the encoded input stays readable until the call ends.
                unsafe { scope.submit(bytes.as_ptr(), len) }
            })
            .unwrap()
    };
    let long = std::format!("{}.png", "a".repeat(60));
    // A budget of 100 bytes holds one 64-byte path, not two.
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 8, 100).with_images(admit(false, 9));
    assert_eq!(submit(&mut stage, &[prepare_cmd(0, &long, 0)]), 0);
    assert_eq!(
        submit(&mut stage, &[prepare_cmd(0, &long, 0)]),
        ERR_NO_SPACE
    );
    assert_eq!(
        stage.take_image_commands().len(),
        1,
        "the refused took no place"
    );
    // A release carries no text and costs no bytes.
    assert_eq!(
        submit(&mut stage, &[WireCommand::ImageRelease { image: 1 }]),
        0
    );
    // Several in one batch count together.
    let mut stage =
        CommandStage::new(&owner, env.settings(), None, 8, 100).with_images(admit(false, 9));
    assert_eq!(
        submit(
            &mut stage,
            &[prepare_cmd(0, &long, 0), prepare_cmd(0, &long, 0)]
        ),
        ERR_NO_SPACE
    );
    assert!(stage.take_image_commands().is_empty());
}

#[test]
fn an_apply_that_cannot_allocate_keeps_its_command_and_never_loses_its_answer() {
    use crate::native_host::shell::tests::FAIL_NTH;
    let storage = MemoryStorage::new();
    png(&storage, "data/a.png", 8, 8);
    for resolves in [true, false] {
        // The source resolves (root 0) or not (root 1: answered with status 6, not found).
        let root = if resolves { 0 } else { 1 };
        for nth in 1..=12 {
            let t = ImageTable::new();
            let cmd = owned(&prepare_cmd(root, "a.png", 0)).unwrap().unwrap();
            FAIL_NTH.with(|n| n.set(nth));
            let first = t.apply(A, &cmd, &Roots);
            FAIL_NTH.with(|n| n.set(0));
            if first.is_err() {
                // Nothing was queued or answered, and the command is still here.
                let inner = t.lock().unwrap();
                assert!(
                    inner.waiting.is_empty() && inner.ready.is_empty(),
                    "nth {nth}"
                );
                drop(inner);
                t.apply(A, &cmd, &Roots).unwrap();
            }
            // Exactly one of: the request is queued, or its answer is.
            let inner = t.lock().unwrap();
            assert_eq!(inner.waiting.len() + inner.ready.len(), 1, "nth {nth}");
            // A source that resolves is queued, or (when the queue entry could not
            // be made) answered with no-memory; one that does not is answered.
            match inner.ready.first() {
                None => assert!(resolves, "nth {nth}"),
                Some(r) if resolves => assert_eq!(r.status, prepare::NO_MEMORY, "nth {nth}"),
                Some(r) => assert_eq!(r.status, prepare::NOT_FOUND, "nth {nth}"),
            }
        }
    }
}

#[test]
fn root_2_reads_the_plugins_install_folder_and_a_missing_file_is_status_6_on_every_root() {
    let storage = MemoryStorage::new();
    // A plugin's bundle holds a picture; a built-in plugin's folder is empty.
    png(&storage, "plugins/demo/art/cat.png", 8, 8);
    png(&storage, "data/here.png", 8, 8);
    let count = |storage: &MemoryStorage| {
        let mut n = 0;
        storage.list("plugins/demo/art", &mut |_| n += 1).unwrap();
        n
    };
    let before = count(&storage);
    let t = ImageTable::new();
    let mut cookie = 0;
    let mut answer = |root: u32, path: &str| {
        cookie += 1;
        let cmd = owned(&WireCommand::ImagePrepare {
            root,
            path,
            side: 8,
            cookie,
            flags: 0,
        })
        .unwrap()
        .unwrap();
        t.apply(A, &cmd, &Roots).unwrap();
        while t.prepare_step(&storage, prepare::Env::x4(8, false)) == Ok(true) {}
        let ready = told(&t).unwrap();
        // The ids are released at once: room for the next.
        if ready.image != 0 {
            t.release(A, ready.image).unwrap();
        }
        (ready.status, ready.image != 0)
    };
    // A bundle hit.
    assert_eq!(answer(2, "art/cat.png"), (prepare::READY, true));
    // Missing: in a folder that has other files, one that does not exist at
    // all (a built-in plugin's), and on the other roots.
    assert_eq!(answer(2, "art/dog.png"), (prepare::NOT_FOUND, false));
    assert_eq!(answer(2, "builtin.png"), (prepare::NOT_FOUND, false));
    assert_eq!(answer(0, "gone.png"), (prepare::NOT_FOUND, false));
    assert_eq!(answer(1, "gone.png"), (prepare::NOT_FOUND, false));
    assert_eq!(answer(0, "here.png"), (prepare::READY, true));
    // No write: the install folder is as it was; the decoded copies are in
    // the image cache only.
    assert_eq!(count(&storage), before);
    assert!(!storage.exists("plugins/demo/art/dog.png"));
}

#[test]
fn the_bundle_path_is_the_one_core_files_resolves() {
    assert_eq!(
        bundle_source("demo", false, "art/cat.png").as_deref(),
        Ok("plugins/demo/art/cat.png")
    );
    for bad in ["", "../x", "a/../b", "/a", "a//b", "a\\b", "a:b", "x.png."] {
        assert_eq!(
            bundle_source("demo", false, bad),
            Err(ERR_INVALID),
            "{bad:?}"
        );
    }
    assert_eq!(
        bundle_source("demo", false, &"p".repeat(600)),
        Err(paperpaw_native::host_glue::ERR_NO_SPACE)
    );
    // An id that is more than one folder name, or leaves no room.
    assert_eq!(bundle_source("a/b", false, "x.png"), Err(ERR_INVALID));
    assert_eq!(
        bundle_source(&"p".repeat(400), false, "x.png"),
        Err(ERR_INVALID)
    );
    // A built-in plugin's folder is empty, whatever a leftover holds.
    assert_eq!(
        bundle_source("demo", true, "art/cat.png"),
        Err(ERR_NOT_FOUND)
    );
    assert_eq!(bundle_source("demo", true, "../x"), Err(ERR_INVALID));
}
