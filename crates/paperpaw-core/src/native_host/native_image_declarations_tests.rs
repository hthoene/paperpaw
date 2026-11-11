//! The widget pack constructed the way a shell loads a plugin: with its image
//! range, through the production mapping entry (`Retained::make_native_stored`).
//! Its declarations (hundreds of settings, widgets and events) are borrowed
//! from the image and counted against `IMAGE_DECLARATIONS_MAX` (512) and
//! `IMAGE_INDEX_MAX_BYTES` (8 KiB); loaded without an image they would be
//! owned copies, bounded by `SPECS_MAX` (128) and `DECLARATIONS_MAX_BYTES`
//! (32 KiB), which is only for loads without an image (tests, `(null, 0)`).
//!
//! Linux x86_64 only: the image is the read-only mapping of the test binary,
//! found through `/proc/self/maps` (the module is gated in `native_host.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
extern crate std;
use super::{
    HostResources,
    registrations::Record,
    retained::Retained,
    shell,
    staged::{IMAGE_DECLARATIONS_MAX, IMAGE_INDEX_MAX_BYTES, SPECS_MAX},
};
use paperpaw_native::host_glue::{ERR_NO_SPACE, Services};
use paperpaw_plugin_widgets::NATIVE_ENTRY;
use std::alloc::Layout;

struct Heap;
impl Services for Heap {
    fn alloc(&self, s: usize, a: usize) -> *mut u8 {
        // SAFETY: host validates the nonzero layout before forwarding.
        unsafe { std::alloc::alloc(Layout::from_size_align(s, a).unwrap()) }
    }
    unsafe fn dealloc(&self, p: *mut u8, s: usize, a: usize) {
        // SAFETY: this allocation was returned by this heap with the same layout.
        unsafe { std::alloc::dealloc(p, Layout::from_size_align(s, a).unwrap()) }
    }
    unsafe fn realloc(&self, p: *mut u8, s: usize, a: usize, n: usize) -> *mut u8 {
        // SAFETY: the allocation belongs to this heap and n is checked by the host.
        unsafe { std::alloc::realloc(p, Layout::from_size_align(s, a).unwrap(), n) }
    }
    fn micros(&self) -> u64 {
        0
    }
}
fn report(_: &Heap, _: &str, s: super::TeardownStep, e: super::Error) {
    std::eprintln!("native teardown {s:?}: {e:?}");
}

/// A non-zero static: placed in the test binary's read-only data, where the
/// pack's declaration batches are placed too.
static ANCHOR: [u8; 16] = [0x5a; 16];

/// The read-only mapping that holds `ANCHOR`: what a shell gives as the image
/// of a plugin (the X4's flash mapping, `.rodata` on the desktop).
pub(super) fn image() -> &'static [u8] {
    let at = ANCHOR.as_ptr() as usize;
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap();
    for line in maps.lines() {
        let mut fields = line.split_whitespace();
        let (Some(range), Some(perms)) = (fields.next(), fields.next()) else {
            continue;
        };
        let (start, end) = range.split_once('-').unwrap();
        let start = usize::from_str_radix(start, 16).unwrap();
        let end = usize::from_str_radix(end, 16).unwrap();
        if (start..end).contains(&at) {
            assert!(
                perms.starts_with('r') && !perms.contains('w'),
                "the anchor is in a mapping that is not read-only: {line}"
            );
            // SAFETY: a readable mapping of the running binary, never unmapped.
            return unsafe { core::slice::from_raw_parts(start as *const u8, end - start) };
        }
    }
    panic!("the anchor is in no mapping");
}

fn declared_services() -> shell::DeclaredServices {
    use paperpaw_native::metadata::{self, Host as AdmissionHost, Profile, Service};
    let services = ["core.registry", "core.ui", "core.library", "core.files"].map(|key| Service {
        key,
        major: 1,
        semantic_revision: 0,
        min_table_bytes: 0,
    });
    let host = AdmissionHost {
        abi_major: 7,
        minor: 1,
        profile: Profile {
            target: "linux-x86_64-sysv64-le",
            entry_layout: 1,
            entry_bytes: 32,
        },
        services: &services,
        features: &[],
    };
    let checked = metadata::check(
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../plugins/widgets/native/metadata.host.bin"
        )),
        &host,
    )
    .unwrap();
    shell::DeclaredServices::try_from_iter(checked.services()).unwrap()
}

/// What the pack declares, as the mapping closure sees it.
#[derive(Debug, Default, PartialEq, Eq)]
struct Counts {
    specs: usize,
    widgets: usize,
    total: usize,
}

fn construct(image: Option<&'static [u8]>) -> Result<(Counts, usize, usize, bool), shell::Error> {
    let image = image.map_or((core::ptr::null(), 0), |image| {
        (image.as_ptr(), image.len())
    });
    // SAFETY: statically linked code lives for the whole test; the image, when
    // given, is a read-only mapping of this binary that stays mapped.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            NATIVE_ENTRY.entry().make.unwrap(),
            "widgets",
            image,
            report,
            HostResources::try_new().unwrap(),
            declared_services(),
        )
    }?;
    let (retained, counts, raw) = Retained::make_native_stored(
        hosted,
        |view| {
            let mut counts = Counts::default();
            view.visit(&mut |record| {
                counts.total += 1;
                match record {
                    Record::SettingSpec(_) => counts.specs += 1,
                    Record::WidgetSpec(_) => counts.widgets += 1,
                    _ => {}
                }
            })?;
            Ok(counts)
        },
        |_, _| {},
    )?;
    let backed = !raw.batches.is_empty() && raw.batches.iter().all(|batch| batch.image.is_some());
    let used = (raw.image_entries, raw.index_bytes);
    drop(retained);
    Ok((counts, used.0, used.1, backed))
}

#[test]
fn the_widget_pack_is_borrowed_from_its_image_within_the_image_limits() {
    let (counts, entries, index_bytes, backed) = construct(Some(image())).expect("image path");
    assert!(backed, "every batch is kept as offsets into the image");
    assert!(counts.widgets > 0 && counts.specs > 0, "{counts:?}");
    assert_eq!(entries, counts.total, "every declaration is image-backed");
    assert!(entries <= IMAGE_DECLARATIONS_MAX, "{entries}");
    assert!(index_bytes <= IMAGE_INDEX_MAX_BYTES, "{index_bytes}");
}

#[test]
fn without_an_image_the_pack_is_staged_as_owned_copies_under_the_smaller_limits() {
    let (counts, ..) = construct(Some(image())).expect("image path");
    let over = counts.specs > SPECS_MAX;
    // Pinned both ways: refused with NoSpace exactly when the pack declares
    // more than SPECS_MAX settings (the pack of #372 does: 131), accepted as
    // owned copies, borrowing nothing, otherwise.
    match construct(None) {
        Err(error) => {
            assert!(over, "refused although {counts:?} fits SPECS_MAX");
            assert!(
                matches!(error, shell::Error::Factory(ERR_NO_SPACE)),
                "{error:?}"
            );
        }
        Ok((same, entries, index_bytes, backed)) => {
            assert!(!over, "accepted although {counts:?} exceeds SPECS_MAX");
            assert_eq!(same, counts);
            assert_eq!((entries, index_bytes, backed), (0, 0, false));
        }
    }
}

/// Construct with the image and hand the stored raw declarations back with
/// the retained owner.
fn construct_kept() -> (
    Retained<Heap, ()>,
    super::raw_declarations::RawDeclarations,
    usize,
) {
    // SAFETY: as in `construct`.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            NATIVE_ENTRY.entry().make.unwrap(),
            "widgets",
            (image().as_ptr(), image().len()),
            report,
            HostResources::try_new().unwrap(),
            declared_services(),
        )
    }
    .unwrap();
    let (retained, total, raw) = Retained::make_native_stored(
        hosted,
        |view| {
            let mut total = 0;
            view.visit(&mut |_| total += 1)?;
            Ok(total)
        },
        |_, _| {},
    )
    .unwrap();
    (retained, raw, total)
}

#[test]
fn after_stop_every_reader_of_an_image_backed_declaration_answers_with_an_error() {
    use paperpaw_native::host_glue::ERR_INVALID;
    let (retained, raw, total) = construct_kept();
    // Live: the owner's image reads every declaration.
    let mut seen = 0;
    raw.visit("widgets", retained.host().image(), &mut |_| seen += 1)
        .unwrap();
    assert_eq!(seen, total);
    retained.host().stop();
    assert!(
        retained.host().image().is_none(),
        "a stopped owner has no image"
    );
    // Stopped: no stale slice is read; the answer is a clean refusal.
    let mut seen = 0;
    assert_eq!(
        raw.visit("widgets", retained.host().image(), &mut |_| seen += 1),
        Err(ERR_INVALID)
    );
    assert_eq!(seen, 0);
    for batch in &raw.batches {
        assert_eq!(batch.view(None), Err(ERR_INVALID));
    }
}

#[test]
fn an_image_that_is_not_the_one_the_batches_were_kept_in_is_refused() {
    use paperpaw_native::host_glue::ERR_INVALID;
    let (retained, raw, _) = construct_kept();
    let live = retained.host().image().unwrap();
    let kept = raw
        .batches
        .iter()
        .filter_map(|batch| batch.image)
        .collect::<std::vec::Vec<_>>();
    assert!(!kept.is_empty());
    let furthest = kept
        .iter()
        .map(|r| r.offset as usize + r.len as usize)
        .max()
        .unwrap();
    for batch in &raw.batches {
        // The same base, cut short before the end of the last batch (the X4's
        // `[0, code_at)` mapping if the range were smaller than at the start):
        // an offset past the end is refused, never read.
        let cut = &live[..furthest - 1];
        let viewed = batch.view(Some(cut));
        let range = batch.image.unwrap();
        if range.offset as usize + range.len as usize > cut.len() {
            assert_eq!(viewed, Err(ERR_INVALID));
        } else {
            assert!(viewed.is_ok());
        }
        // Another mapping at another address is no image of this batch.
        assert_eq!(batch.view(Some(&live[1..])), Err(ERR_INVALID));
    }
}

/// The (kept, copied) settings of the pack for `table`.
fn settings_paths(table: &'static [paperpaw_sdk::SettingSpec]) -> (usize, usize) {
    // SAFETY: as in `construct`.
    let hosted = unsafe {
        shell::Hosted::new(
            Heap,
            (),
            NATIVE_ENTRY.entry().make.unwrap(),
            "widgets",
            (image().as_ptr(), image().len()),
            report,
            HostResources::try_new().unwrap(),
            declared_services(),
        )
    }
    .unwrap();
    let (_retained, paths, _raw) = Retained::make_native_stored(
        hosted,
        |view| super::native_install::settings_paths(view, table),
        |_, _| {},
    )
    .unwrap();
    paths
}

#[test]
fn every_setting_of_the_widget_pack_takes_its_table_entry() {
    // The table the build script makes of the declarations: a spec that
    // silently fell back to a copy would lose the saving unseen, so none may.
    let table = paperpaw_plugin_widgets::NATIVE_SETTINGS;
    let (kept, copied) = settings_paths(table);
    assert!(kept > SPECS_MAX, "{kept} settings kept, {copied} copied");
    assert_eq!(copied, 0, "{copied} settings of the pack are copies");
    assert_eq!(kept, table.len(), "every entry of the table is taken once");
    // Without a table every one is a copy, as before.
    assert_eq!(settings_paths(&[]), (0, kept));
}

#[test]
fn a_table_entry_that_differs_from_the_declaration_is_not_taken() {
    // The first entry's default changed: that setting is copied, the rest are
    // kept, and nothing is taken without the comparison.
    let table = paperpaw_plugin_widgets::NATIVE_SETTINGS;
    let mut changed = table.to_vec();
    let first = changed
        .iter()
        .position(|spec| matches!(spec.default, paperpaw_sdk::SettingValue::Bool(_)))
        .expect("the pack has a switch");
    let paperpaw_sdk::SettingValue::Bool(was) = changed[first].default else {
        unreachable!("found as a switch");
    };
    changed[first].default = paperpaw_sdk::SettingValue::Bool(!was);
    let changed: &'static [paperpaw_sdk::SettingSpec] = std::boxed::Box::leak(changed.into());
    let (kept, copied) = settings_paths(changed);
    assert_eq!((kept + copied, copied), (table.len(), 1));
}

#[test]
fn the_table_defines_the_same_settings_as_the_copies_do() {
    use paperpaw_sdk::Settings;
    let table = paperpaw_plugin_widgets::NATIVE_SETTINGS;
    let mut kept = Settings::new();
    let mut copied = Settings::new();
    kept.try_reserve_with_fixed(0, table.len(), 0).unwrap();
    copied.try_reserve(table.len(), 0).unwrap();
    for at in 0..table.len() {
        kept.define_fixed(table, at).unwrap();
        copied.define(table[at].clone()).unwrap();
    }
    let ids = |settings: &Settings| {
        settings
            .specs()
            .map(|spec| std::string::String::from(spec.id.as_str()))
            .collect::<std::vec::Vec<_>>()
    };
    // The same settings in the same order, with the same values.
    assert_eq!(ids(&kept), ids(&copied));
    for spec in copied.specs() {
        let other = kept.spec(spec.id.as_str()).unwrap();
        assert!(super::fixed_settings::same_setting(spec, other));
        assert_eq!(kept.get(spec.id.as_str()), copied.get(spec.id.as_str()));
        assert_eq!(kept.shows(other), copied.shows(spec));
    }
    // Forgetting the plugin leaves nothing of either.
    kept.remove_plugin("widgets");
    copied.remove_plugin("widgets");
    assert_eq!(kept.specs().count(), 0);
    assert_eq!(copied.specs().count(), 0);
}
