//! The flat-instance step (#228): an interface-7 library as the kernel's
//! `FlatInstance`: its registrations visited, events dispatched through the
//! pinned owner, stopped means the owner's error, declarations refused (not
//! dropped), the read-only image, and the release order.
use super::super::dev::{Loaded, Refusal, load_flat};
use super::foreign::{fixture_custom, fixture_linked};
use super::*;
use paperpaw_core::native_host::Error as HostError;
use paperpaw_core::native_host::flat::{DropReason, FlatInstance, FlatRegistration};
use paperpaw_core::native_host::registrations::{
    self, EVENT_STARTED_TAG, REGISTRATION_MAX_BYTES, Record, Registration, SettingSpec,
    records_encode,
};

const TOKEN: u32 = 17;

fn tick_bytes() -> Vec<u8> {
    let mut bytes = [0; 64];
    let len = registrations::tick_encode(41, &mut bytes).unwrap();
    bytes[..len].to_vec()
}

fn batch(records: &[Registration]) -> Vec<u8> {
    let mut bytes = vec![0; REGISTRATION_MAX_BYTES];
    let len = registrations::registrations_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    bytes
}

/// A library registering the started event (the fixture's callback accepts the
/// bytes of a tick event only, so the payload tells whether it was delivered).
fn started_library() -> Vec<u8> {
    let valid = batch(&[Registration::Event {
        event_tag: EVENT_STARTED_TAG,
        token: TOKEN,
    }]);
    let library = fixture_custom(valid, &[], false, "", "");
    let bytes = std::fs::read(&library.path).unwrap();
    drop(library);
    bytes
}

/// The distinct private copies of plugin `id` mapped now, by path.
fn mapped_copies(id: &str) -> Vec<String> {
    let mut paths: Vec<String> = std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .filter(|line| line.contains("paperpaw-abi7") && line.contains(&format!("/{id}-")))
        .filter_map(|line| line.split_whitespace().nth(5).map(str::to_owned))
        .collect();
    paths.sort();
    paths.dedup();
    paths
}

fn mapped_copy(id: &str) -> bool {
    std::fs::read_to_string("/proc/self/maps")
        .unwrap()
        .lines()
        .any(|line| line.contains("paperpaw-abi7") && line.contains(&format!("/{id}-")))
}

#[test]
fn registrations_are_visited_in_batch_order_with_events_apart_from_other_kinds() {
    let valid = batch(&[
        Registration::Event {
            event_tag: EVENT_STARTED_TAG,
            token: TOKEN,
        },
        Registration::Draw { token: TOKEN },
        Registration::Event {
            event_tag: registrations::EVENT_TICK_TAG,
            token: TOKEN,
        },
    ]);
    let library = fixture_custom(valid, &[], false, "", "");
    let bytes = std::fs::read(&library.path).unwrap();
    drop(library);
    let instance = load_flat("flatvisit", &bytes).unwrap();
    let mut seen = Vec::new();
    instance.registrations(&mut |r| seen.push(r));
    assert_eq!(
        seen,
        [
            FlatRegistration::Event {
                event_tag: EVENT_STARTED_TAG,
                index: 0
            },
            FlatRegistration::Other { record_tag: 2 },
            FlatRegistration::Event {
                event_tag: registrations::EVENT_TICK_TAG,
                index: 2
            },
        ]
    );
    // Visited again, the same: the kernel reads them at construction and at register.
    let mut again = Vec::new();
    instance.registrations(&mut |r| again.push(r));
    assert_eq!(again, seen);
    drop(instance);
    assert!(!mapped_copy("flatvisit"));
}

#[test]
fn an_event_is_dispatched_with_its_payload_and_the_plugins_status_is_returned() {
    let instance = load_flat("flatevent", &started_library()).unwrap();
    let depth = DispatchDepth::new();
    // The callback accepts exactly the bytes it expects.
    assert_eq!(
        instance.dispatch_event(0, &tick_bytes(), &depth),
        Ok(host_glue::OK)
    );
    // Other bytes: the plugin's refusal comes back as its status, not as a panic.
    assert_eq!(
        instance.dispatch_event(0, &[1, 2, 3], &depth),
        Ok(host_glue::ERR_INVALID)
    );
    // An index that was never registered is the owner's error.
    assert_eq!(
        instance.dispatch_event(9, &tick_bytes(), &depth),
        Err(HostError::State)
    );
    // The environment scope is the owner's own, stable between calls.
    let first = std::ptr::from_ref(instance.environment().get_ref());
    let second = std::ptr::from_ref(instance.environment().get_ref());
    assert_eq!(first, second);
}

#[test]
fn a_dropped_event_is_logged_and_never_runs_plugin_code_or_panics() {
    let instance = load_flat("flatdrop", &started_library()).unwrap();
    for why in [
        DropReason::Busy,
        DropReason::State,
        DropReason::Producer,
        DropReason::Plugin(-3),
    ] {
        instance.event_dropped(EVENT_STARTED_TAG, why);
    }
    assert_eq!(
        instance.dispatch_event(0, &tick_bytes(), &DispatchDepth::new()),
        Ok(host_glue::OK),
        "still alive and answering"
    );
}

#[test]
fn a_stopped_instance_answers_with_the_owners_error_and_gives_no_image() {
    let bytes = started_library();
    let loaded = Loaded::from_bytes("flatstop", &bytes).unwrap();
    let depth = DispatchDepth::new();
    assert_eq!(
        loaded.dispatch_event(0, &tick_bytes(), &depth),
        Ok(host_glue::OK)
    );
    assert!(loaded.image().is_some_and(|image| !image.is_empty()));
    loaded.stop();
    // Referenced still (the kernel's subscriptions hold it), but stopped: the
    // owner's error, and nothing of the library is read.
    assert_eq!(
        loaded.dispatch_event(0, &tick_bytes(), &depth),
        Err(HostError::State)
    );
    assert_eq!(loaded.image(), None);
    loaded.stop();
    assert_eq!(loaded.image(), None, "stopping twice is the same");
    assert!(mapped_copy("flatstop"), "still mapped until it is dropped");
    drop(loaded);
    assert!(!mapped_copy("flatstop"), "unmapped with the drop, once");
}

#[test]
fn the_image_is_the_read_only_data_the_library_is_mapped_with() {
    let bytes = started_library();
    let loaded = Loaded::from_bytes("flatimage", &bytes).unwrap();
    let image = loaded.image().unwrap();
    // The metadata record is a read-only static of the library: it lies in the image.
    let metadata = crate::native_runtime::tests::metadata_bytes();
    assert!(
        image
            .windows(metadata.len())
            .any(|window| window == metadata.as_slice()),
        "the metadata record is in the read-only range"
    );
    // The range is what the ELF says: a segment neither writable nor executable.
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    let start = image.as_ptr() as usize;
    let line = maps
        .lines()
        .find(|line| {
            let range = line.split_whitespace().next().unwrap();
            let (from, to) = range.split_once('-').unwrap();
            usize::from_str_radix(from, 16).unwrap() <= start
                && start < usize::from_str_radix(to, 16).unwrap()
        })
        .unwrap();
    let perms = line.split_whitespace().nth(1).unwrap();
    assert!(perms.starts_with("r--"), "read-only mapping: {line}");
}

#[test]
fn declarations_are_refused_not_dropped_and_the_library_is_released() {
    let mut valid = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(
        &[Record::SettingSpec(SettingSpec {
            name: "s",
            kind: 3,
            flags: 0,
            group: "flatdecl.main",
            title: "title",
            help: "",
            default: &[1, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, b'x'],
            min: 0,
            max: 0,
            choices: "",
        })],
        &mut valid,
    )
    .unwrap();
    valid.truncate(len);
    let library = fixture_custom(valid, &[], false, "", "");
    let bytes = std::fs::read(&library.path).unwrap();
    drop(library);
    // The group is not declared by the library, so the folder's id decides:
    // a library that stages cleanly under its own group is refused at the apply
    // step instead. Either way, a declaring library is never loaded without them.
    let refused = load_flat("flatdecl", &bytes).err().unwrap();
    assert!(
        matches!(
            refused,
            Refusal::Declared(_) | Refusal::Lifecycle(_) | Refusal::Runtime(_)
        ),
        "{refused:?}"
    );
    assert!(!mapped_copy("flatdecl"), "released after the refusal");
    // The same library under an id its declaration names: staged, and refused
    // visibly: for now by the kernel (declarations in the image cannot be read
    // back until the draft-4 views are wired), later by this loader if it
    // cannot apply them.
    let own = load_flat("other", &bytes).err().unwrap();
    assert!(
        matches!(&own, Refusal::Lifecycle(why) if why.contains("DeclarationViewsUnavailable")),
        "{own:?}"
    );
    assert!(!mapped_copy("other"));
}

#[test]
fn a_library_that_is_no_interface_7_one_or_has_no_id_is_refused_cleanly() {
    assert!(matches!(
        load_flat("flatjunk", b"junk").err(),
        Some(Refusal::Kind(_))
    ));
    assert!(matches!(
        load_flat("Not An Id", &started_library()).err(),
        Some(Refusal::Id)
    ));
}

/// Rustc's own lld, called through the compiler driver (`-fuse-ld=lld`).
fn lld_args() -> Vec<String> {
    let out = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    let sysroot = String::from_utf8(out.stdout).unwrap();
    let rustlib = std::path::Path::new(sysroot.trim()).join("lib/rustlib");
    let dir = std::fs::read_dir(&rustlib)
        .unwrap()
        .map(|entry| entry.unwrap().path().join("bin/gcc-ld"))
        .find(|dir| dir.join("ld.lld").exists())
        .unwrap_or_else(|| panic!("rustc ships no ld.lld under {rustlib:?}"));
    vec![
        "-Clink-arg=-fuse-ld=lld".to_owned(),
        format!("-Clink-arg=-B{}", dir.display()),
    ]
}

#[test]
fn the_image_holds_the_metadata_whichever_linker_laid_the_segments_out() {
    let valid = batch(&[Registration::Event {
        event_tag: EVENT_STARTED_TAG,
        token: TOKEN,
    }]);
    let metadata = crate::native_runtime::tests::metadata_bytes();
    for (name, link) in [
        ("gnu", vec!["-Clink-arg=-fuse-ld=bfd".to_owned()]),
        ("lld", lld_args()),
    ] {
        let library = fixture_linked(valid.clone(), &[], false, "", "", &link);
        let bytes = std::fs::read(&library.path).unwrap();
        drop(library);
        // The linker is the one asked for (it names itself in `.comment`).
        let lld = bytes.windows(10).any(|window| window == b"Linker: LL");
        assert_eq!(lld, name == "lld", "{name}: linked by the linker asked for");
        let id = format!("flat{name}");
        let loaded = Loaded::from_bytes(&id, &bytes).unwrap();
        let image = loaded.image().unwrap();
        assert!(
            image
                .windows(metadata.len())
                .any(|window| window == metadata.as_slice()),
            "{name}: the metadata record is in the image"
        );
        // Read-only: not the segment of the code.
        assert!(!image.is_empty(), "{name}");
    }
}

/// The teardown order on an update and an uninstall: the old copy stays
/// mapped until its own release, the new one is a library of its own, and the
/// last release leaves no mapping and no copy behind.
#[test]
fn update_and_uninstall_unmap_each_copy_only_after_its_release() {
    let first = started_library();
    let second = {
        let valid = batch(&[
            Registration::Event {
                event_tag: EVENT_STARTED_TAG,
                token: TOKEN,
            },
            Registration::Draw { token: TOKEN },
        ]);
        let library = fixture_custom(valid, &[], false, "", "");
        let bytes = std::fs::read(&library.path).unwrap();
        drop(library);
        bytes
    };
    assert_ne!(first, second);
    let old = Loaded::from_bytes("flatupdate", &first).unwrap();
    let old_copy = mapped_copies("flatupdate");
    assert_eq!(old_copy.len(), 1, "the old library: {old_copy:?}");
    // The update: the kernel stops the old one and loads the new one; the old
    // library stays mapped while anything holds its instance.
    old.stop();
    let new = Loaded::from_bytes("flatupdate", &second).unwrap();
    let both = mapped_copies("flatupdate");
    assert_eq!(both.len(), 2, "two distinct mappings: {both:?}");
    assert!(both.contains(&old_copy[0]));
    let new_copy = both.iter().find(|p| **p != old_copy[0]).unwrap().clone();
    let mut seen = Vec::new();
    new.registrations(&mut |r| seen.push(r));
    assert_eq!(seen.len(), 2, "the new library's registrations");
    drop(old);
    assert_eq!(
        mapped_copies("flatupdate"),
        [new_copy.clone()],
        "the old copy is unmapped by its own release, the new one stays"
    );
    assert_eq!(
        new.dispatch_event(0, &tick_bytes(), &DispatchDepth::new()),
        Ok(host_glue::OK),
        "and runs"
    );
    // The uninstall: stopped, then released.
    new.stop();
    drop(new);
    assert!(mapped_copies("flatupdate").is_empty(), "no mapping left");
    assert!(
        !std::path::Path::new(&new_copy).exists(),
        "and no copy file left"
    );
}

/// A load that fails after the library ran code (`make` done, commit refused)
/// releases what it made and unmaps; a retry is a clean load.
#[test]
fn a_failed_load_unmaps_and_a_retry_is_a_clean_load() {
    // Declaring under the folder's own id: refused at commit.
    let mut valid = vec![0; REGISTRATION_MAX_BYTES];
    let len = records_encode(
        &[Record::SettingSpec(SettingSpec {
            name: "s",
            kind: 3,
            flags: 0,
            group: "flatretry.main",
            title: "title",
            help: "",
            default: &[1, 0, 0, 0, 3, 0, 0, 0, 1, 0, 0, 0, b'x'],
            min: 0,
            max: 0,
            choices: "",
        })],
        &mut valid,
    )
    .unwrap();
    valid.truncate(len);
    let library = fixture_custom(valid, &[], false, "", "");
    let bad = std::fs::read(&library.path).unwrap();
    drop(library);
    assert!(load_flat("flatretry", &bad).is_err());
    assert!(!mapped_copy("flatretry"), "unmapped after the refusal");
    let good = load_flat("flatretry", &started_library()).unwrap();
    assert!(mapped_copy("flatretry"));
    drop(good);
    assert!(!mapped_copy("flatretry"));
}
