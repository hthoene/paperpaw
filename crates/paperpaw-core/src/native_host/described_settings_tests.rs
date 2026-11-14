//! The widget pack's settings described from its image (#248): admitted with
//! its image and no table, the kernel keeps none of its specs, reads them as
//! the table's and lets go of what a round made of them.
//!
//! Linux x86_64 only: the image is the test binary's read-only mapping
//! (`native_image_declarations_tests`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "fixture failures fail the test"
)]
extern crate std;
use super::super::native_image_declarations_tests::{Heap, declared_services, image, report};
use super::super::native_instances::{NativeInstance, NativeLease};
use super::super::native_screen::Dispatch;
use super::super::registrations::{self as wire, Record};
use super::super::shell::tests::{FAIL_NTH, FAIL_SIZE, HEAP_LIVE};
use super::super::{FixedTables, HostResources, flat::FlatDepth, shell};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_NO_MEMORY, ERR_UNSUPPORTED};
use paperpaw_plugin_widgets::{NATIVE_ENTRY, NATIVE_GROUPS, NATIVE_SETTINGS};
use paperpaw_sdk::{Pick, SettingError, SettingValue, Settings};
use std::rc::Rc;
use std::string::String;
use std::vec::Vec;

fn hosted() -> shell::Hosted<Heap, ()> {
    // SAFETY: statically linked code lives for the whole test; the image is a
    // read-only mapping of this binary that stays mapped.
    unsafe {
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
    .unwrap()
}

/// A kernel with the pack: its settings described from the image, or taken
/// from its table in the program (what a linked build does).
pub(in crate::native_host) fn kernel(table: bool) -> crate::Kernel {
    configured(table, "")
}

/// [`kernel`] with `config` as the shell's configuration, read at the build
/// (before the pack registers).
pub(in crate::native_host) fn configured(table: bool, config: &str) -> crate::Kernel {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .user_config(config)
        .build()
        .unwrap();
    add_pack(&mut kernel, table);
    kernel
}

/// Registers the pack with `kernel`, as [`kernel`] says.
pub(in crate::native_host) fn add_pack(kernel: &mut crate::Kernel, table: bool) {
    let depth = FlatDepth::try_new().unwrap();
    let fixed = match table {
        true => FixedTables::new(NATIVE_SETTINGS, NATIVE_GROUPS),
        false => FixedTables::NONE,
    };
    kernel
        .register_native_fixed(hosted(), &depth, |_, _, _, _| {}, fixed, None)
        .unwrap();
}

/// The pack admitted and nothing more: without the steps that follow an
/// admission (the font choices, the widgets' font settings).
pub(in crate::native_host) fn admit_pack(kernel: &mut crate::Kernel) {
    let depth = FlatDepth::try_new().unwrap();
    kernel
        .admit_native(hosted(), &depth, None, |_, _, _, _| {})
        .unwrap();
}

fn ids(settings: &Settings) -> Vec<String> {
    let mut ids = Vec::new();
    settings
        .each(Pick::All, |spec| ids.push(String::from(spec.id.as_str())))
        .unwrap();
    ids
}

/// The pack's settings the kernel holds a spec of.
pub(in crate::native_host) fn held(settings: &Settings) -> usize {
    let table = |id: &str| NATIVE_SETTINGS.iter().any(|spec| spec.id.as_str() == id);
    settings
        .kept_specs()
        .filter(|spec| table(spec.id.as_str()))
        .count()
}

#[test]
fn the_pack_s_settings_are_the_table_s_and_none_of_them_is_held() {
    let described = kernel(false);
    let table = kernel(true);
    let (lazy, eager) = (&described.shared.settings, &table.shared.settings);
    // Nothing of them on the heap: no spec held, none made so far.
    assert_eq!(held(lazy), 0);
    assert!(!lazy.any_made());
    assert_eq!(held(eager), NATIVE_SETTINGS.len());
    // The same settings in the same order, the kernel's own among them.
    assert_eq!(ids(lazy), ids(eager));
    for wanted in NATIVE_SETTINGS {
        let id = wanted.id.as_str();
        assert!(lazy.defines(id), "{id}");
        assert_eq!(lazy.get(id), eager.get(id), "{id}");
        let spec = lazy.try_spec(id).unwrap().expect(id);
        assert!(
            super::super::fixed_settings::same_setting(spec, wanted),
            "{id}"
        );
        assert_eq!(lazy.shows(spec), eager.shows(wanted), "{id}");
    }
}

/// A value the configuration has for a described setting is set when its
/// plugin registers, which reads the setting's spec. The spec is let go with
/// the publication: at the start plugin after plugin registers with no round
/// between, and each kept spec would stay through the whole of it.
#[test]
fn what_a_configured_value_made_of_them_is_let_go_with_the_publication() {
    let (wanted, on) = NATIVE_SETTINGS
        .iter()
        .find_map(|spec| match spec.default {
            SettingValue::Bool(on) => Some((spec, on)),
            _ => None,
        })
        .unwrap();
    let (plugin, name) = wanted.id.as_str().split_once('.').unwrap();
    let value = if on { "off" } else { "on" };
    let kernel = configured(false, &std::format!("[{plugin}]\n{name} = {value}\n"));
    // Set from the file, so its spec was read; and nothing of it is kept.
    let settings = &kernel.shared.settings;
    assert_eq!(
        settings.get(wanted.id.as_str()),
        Some(&SettingValue::Bool(!on))
    );
    assert!(!settings.any_made());
    assert!(!settings.went_short());
}

#[test]
fn what_a_round_made_of_them_is_let_go_at_its_end() {
    let mut kernel = kernel(false);
    let id = NATIVE_SETTINGS[0].id.as_str();
    assert!(kernel.shared.settings.spec(id).is_some());
    assert!(kernel.shared.settings.any_made());
    let _ = kernel.finish();
    assert!(!kernel.shared.settings.any_made());
    // And made again when asked for.
    assert!(kernel.shared.settings.spec(id).is_some());
}

#[test]
fn without_room_for_a_spec_a_change_is_refused_and_nothing_changes() {
    let mut kernel = kernel(false);
    let wanted = NATIVE_SETTINGS
        .iter()
        .find(|spec| matches!(spec.default, SettingValue::Bool(_)))
        .expect("the pack has a switch");
    let id = wanted.id.as_str();
    let SettingValue::Bool(was) = wanted.default else {
        unreachable!("found as a switch");
    };
    let settings = &mut kernel.shared.settings;
    // Changed once, so that the value has its place; then changed back with
    // every allocation of reading its spec refused in turn: each is told as
    // no room, never as an unknown setting, and leaves the value.
    assert_eq!(settings.set(id, SettingValue::Bool(!was)), Ok(()));
    settings.rest();
    let mut tried = 0;
    for nth in 1..64 {
        FAIL_NTH.with(|fail| fail.set(nth));
        let result = settings.set(id, SettingValue::Bool(was));
        let fired = FAIL_NTH.with(|fail| fail.replace(0)) == 0;
        if !fired {
            assert_eq!(result, Ok(()));
            break;
        }
        tried += 1;
        assert_eq!(result, Err(SettingError::Full), "allocation {nth}");
        assert_eq!(settings.bool(id), !was, "allocation {nth}");
        assert!(settings.defines(id));
    }
    assert!(tried > 0, "reading the spec took no allocation");
    assert_eq!(settings.bool(id), was);
    // The spec asked for by itself (kept until the round ends, so one more
    // allocation): the same answer, and nothing of it left after the round.
    for nth in 1..=tried + 1 {
        FAIL_NTH.with(|fail| fail.set(nth));
        let spec = settings.try_spec(id).map(|spec| spec.is_some());
        FAIL_NTH.with(|fail| fail.set(0));
        assert_eq!(spec, Err(SettingError::Full), "allocation {nth}");
        settings.rest();
        assert!(!settings.any_made());
    }
    assert!(settings.try_spec(id).unwrap().is_some());
}

/// A plugin that is nothing but an image the test can change or take away.
pub(in crate::native_host) struct Shown {
    image: RefCell<Vec<u8>>,
    live: Cell<bool>,
}
impl Dispatch for Rc<Shown> {
    fn event(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn input(&self, _: u32, _: &[u8], _: &paperpaw_sdk::Env<'_>) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn draw(
        &self,
        _: u32,
        _: &wire::DrawFrame<'_>,
        _: &paperpaw_sdk::Env<'_>,
        _: &mut paperpaw_graphics::Canvas<'_>,
    ) -> Result<i32, i32> {
        Err(ERR_UNSUPPORTED)
    }
    fn dropped(&self, _: u32, _: i32) {}
}
impl NativeInstance for Rc<Shown> {
    fn running(&self) -> bool {
        self.live.get()
    }
    fn stop(&self) {
        self.live.set(false);
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        let image = self.image.borrow();
        visit(self.live.get().then_some(&image[..]));
    }
}

/// `true` as a setting's default on the wire.
const ON: &[u8] = &[1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1];

pub(in crate::native_host) fn switch<'a>(name: &'a str, title: &'a str, flags: u32) -> Record<'a> {
    Record::SettingSpec(wire::SettingSpec {
        name,
        kind: 0,
        flags,
        group: "probe.group",
        title,
        help: "",
        default: ON,
        min: 0,
        max: 0,
        choices: "",
    })
}

/// The batch of `records` behind some bytes that are no record, and where
/// each record's frame starts.
fn batch(records: &[Record<'_>]) -> (Vec<u8>, Vec<u32>) {
    let mut bytes = std::vec![0; wire::REGISTRATION_MAX_BYTES];
    let len = wire::records_encode(records, &mut bytes).unwrap();
    bytes.truncate(len);
    let lead = 5;
    let places = (0..records.len())
        .map(|at| {
            let rest = super::super::raw_declarations::record_at(&bytes, at).unwrap();
            u32::try_from(lead + bytes.len() - rest.len()).unwrap()
        })
        .collect();
    let mut image = std::vec![0xee; lead];
    image.extend_from_slice(&bytes);
    (image, places)
}

/// A plugin `owner` with the settings `records` in its image, defined in
/// `settings` as what it describes from it.
pub(in crate::native_host) fn define_plugin(
    settings: &mut Settings,
    owner: &'static str,
    records: &[Record<'_>],
) -> Rc<Shown> {
    let (shown, described) = plugin_described(owner, records);
    define(settings, described);
    shown
}

fn plugin_described(
    owner: &'static str,
    records: &[Record<'_>],
) -> (Rc<Shown>, paperpaw_sdk::DescribedSettings) {
    let (image, places) = batch(records);
    let shown = Rc::new(Shown {
        image: RefCell::new(image),
        live: Cell::new(true),
    });
    let lease = NativeLease::try_new(Rc::clone(&shown)).unwrap();
    let owner = paperpaw_sdk::Id::new_static(owner);
    let described = super::described(&owner, &lease, places).unwrap();
    (shown, described)
}

/// A plugin `probe` with the switches `one`, `three` (both scoped) and `two`
/// in its image, and what describes them from it.
fn probe_described() -> (Rc<Shown>, paperpaw_sdk::DescribedSettings) {
    let records = [
        switch("one", "First", 1),
        switch("two", "Other", 0),
        switch("three", "Third", 1),
    ];
    plugin_described("probe", &records)
}

fn define(settings: &mut Settings, described: paperpaw_sdk::DescribedSettings) {
    settings.check_described(&described).unwrap();
    settings.try_reserve_described(described.len(), 0).unwrap();
    settings.define_described(described).unwrap();
}

/// [`probe_described`], and settings that have them.
pub(in crate::native_host) fn probe() -> (Rc<Shown>, Settings) {
    let (shown, described) = probe_described();
    let mut settings = Settings::new();
    define(&mut settings, described);
    (shown, settings)
}

#[test]
fn a_spec_is_read_from_the_image_anew_and_owns_all_its_texts() {
    let (shown, mut settings) = probe();
    let spec = settings.spec("probe.one").unwrap();
    assert_eq!(
        (spec.title.as_str(), spec.group.as_str(), spec.scoped),
        ("First", "probe.group", true)
    );
    assert_eq!(spec.default, SettingValue::Bool(true));
    // The image changes under the spec made: it keeps its own texts.
    let at = {
        let image = shown.image.borrow();
        image
            .windows(5)
            .position(|bytes| bytes == b"First")
            .unwrap()
    };
    shown.image.borrow_mut()[at..at + 5].copy_from_slice(b"Again");
    assert_eq!(spec.title.as_str(), "First");
    // Asked for after the round, it is what the image says now.
    settings.rest();
    assert_eq!(settings.spec("probe.one").unwrap().title.as_str(), "Again");
}

#[test]
fn a_stopped_plugin_s_settings_have_no_spec_and_are_never_no_room() {
    let (shown, mut settings) = probe();
    shown.stop();
    for id in ["probe.one", "probe.two"] {
        assert_eq!(settings.try_spec(id).map(|spec| spec.is_some()), Ok(false));
        assert_eq!(
            settings.set(id, SettingValue::Bool(false)),
            Err(SettingError::Unknown)
        );
    }
    assert_eq!(settings.each(Pick::All, |_| panic!("a spec")), Ok(()));
    // Its image back (a plugin made again), they are what they were.
    shown.live.set(true);
    assert!(settings.spec("probe.two").is_some());
    assert_eq!(settings.set("probe.two", SettingValue::Bool(false)), Ok(()));
}

#[test]
fn a_record_that_is_not_the_one_described_is_no_setting() {
    let (shown, settings) = probe();
    // Another setting where `one` was, and no record where `two` was.
    let (image, places) = batch(&[switch("eins", "First", 1), switch("two", "Other", 0)]);
    let cut = places[1] as usize + 2;
    *shown.image.borrow_mut() = image[..cut].to_vec();
    assert_eq!(
        settings.try_spec("probe.one").map(|s| s.is_some()),
        Ok(false)
    );
    assert_eq!(
        settings.try_spec("probe.two").map(|s| s.is_some()),
        Ok(false)
    );
    assert!(settings.spec("probe.eins").is_none());
    // A walk asks each place for its spec, by no name: it gives none either.
    let walked = settings.each(Pick::All, |spec| panic!("{}", spec.id.as_str()));
    assert_eq!(walked, Ok(()));
    // Of another kind than the default kept: not that setting either.
    let (image, _) = batch(&[
        Record::SettingSpec(wire::SettingSpec {
            name: "one",
            kind: 3,
            flags: 1,
            group: "probe.group",
            title: "First",
            help: "",
            default: ON,
            min: 0,
            max: 0,
            choices: "",
        }),
        switch("two", "Other", 0),
    ]);
    *shown.image.borrow_mut() = image;
    assert_eq!(
        settings.try_spec("probe.one").map(|s| s.is_some()),
        Ok(false)
    );
}

#[test]
fn describing_without_room_is_told_as_that_and_leaves_nothing() {
    let (image, places) = batch(&[switch("one", "First", 1), switch("two", "Other", 0)]);
    let shown = Rc::new(Shown {
        image: RefCell::new(image),
        live: Cell::new(true),
    });
    let lease = NativeLease::try_new(Rc::clone(&shown)).unwrap();
    let owner = paperpaw_sdk::Id::new_static("probe");
    let mut refused = 0;
    for nth in 1..256 {
        // The places are the call's: refused, it lets them go too.
        let held = HEAP_LIVE.with(Cell::get);
        let mut copy = Vec::new();
        copy.extend_from_slice(&places);
        FAIL_NTH.with(|fail| fail.set(nth));
        let described = super::described(&owner, &lease, copy);
        let fired = FAIL_NTH.with(|fail| fail.replace(0)) == 0;
        if !fired {
            assert_eq!(described.unwrap().len(), 2);
            break;
        }
        assert_eq!(described.err(), Some(ERR_NO_MEMORY), "allocation {nth}");
        assert_eq!(HEAP_LIVE.with(Cell::get), held, "allocation {nth}");
        refused += 1;
    }
    assert!(refused > 3, "{refused} allocations refused");
}

const BOOK: &str = "books/a/settings.ini";
const BOOK_SETTINGS: &str = "[core]\nscope-only = on\n[probe]\none = off\nthree = off\n";

/// A kernel with `probe`'s settings and a book's settings file that sets
/// two of them; `config` is the shell's configuration.
fn book_kernel(config: &str) -> (Rc<Shown>, crate::Kernel) {
    let storage = paperpaw_sdk::MemoryStorage::new().with(BOOK, BOOK_SETTINGS);
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(storage)
        .user_config(config)
        .build()
        .unwrap();
    let (shown, described) = probe_described();
    define(&mut kernel.shared.settings, described);
    (shown, kernel)
}

/// The next spec made of a described setting finds no room.
pub(in crate::native_host) fn no_room_for_a_spec() {
    FAIL_SIZE.with(|size| size.set(size_of::<paperpaw_sdk::SettingSpec>()));
}

fn warned(kernel: &mut crate::Kernel, of: &str) -> bool {
    let warnings = kernel.take_warnings();
    warnings.iter().any(|w| w.message.contains(of))
}

#[test]
fn a_book_s_settings_read_without_room_are_not_used_and_not_written_over() {
    let off = Some(&SettingValue::Bool(false));
    let (_shown, mut kernel) = book_kernel("");
    kernel.set_scope(Some(String::from(BOOK)));
    let settings = &kernel.shared.settings;
    assert_eq!(
        (settings.get("probe.one"), settings.get("probe.three")),
        (off, off)
    );

    // No room for the first one's spec: the other alone is not the book's
    // settings, and saved it would take the first out of the file.
    let (_shown, mut kernel) = book_kernel("");
    no_room_for_a_spec();
    kernel.set_scope(Some(String::from(BOOK)));
    assert_eq!(
        FAIL_SIZE.with(|size| size.replace(0)),
        0,
        "no spec was made"
    );
    assert!(warned(&mut kernel, "no memory to read its settings"));
    let on = Some(&SettingValue::Bool(true));
    let settings = &kernel.shared.settings;
    assert_eq!(
        (settings.get("probe.one"), settings.get("probe.three")),
        (on, on)
    );
    let three = paperpaw_sdk::Id::new_static("probe.three");
    kernel.set_in_scope(&three, SettingValue::Bool(true));
    assert!(warned(&mut kernel, "settings not saved to it"));
    kernel.shared.recall.clear();
    let stored = paperpaw_sdk::read_file(kernel.storage(), BOOK, 1 << 20).unwrap();
    assert_eq!(stored, BOOK_SETTINGS.as_bytes());
}

#[test]
fn a_value_kept_for_a_setting_is_not_called_unknown_for_lack_of_room() {
    let config = "[probe]\ntwo = off\n";
    let (_shown, mut kernel) = book_kernel(config);
    kernel.apply_late_config();
    assert_eq!(
        kernel.shared.settings.get("probe.two"),
        Some(&SettingValue::Bool(false))
    );
    assert!(!warned(&mut kernel, "probe.two"));

    let (_shown, mut kernel) = book_kernel(config);
    no_room_for_a_spec();
    kernel.apply_late_config();
    assert_eq!(
        FAIL_SIZE.with(|size| size.replace(0)),
        0,
        "no spec was made"
    );
    assert_eq!(
        kernel.shared.settings.get("probe.two"),
        Some(&SettingValue::Bool(true))
    );
    let warnings = kernel.take_warnings();
    let about: Vec<_> = warnings
        .iter()
        .filter(|w| w.message.contains("probe.two"))
        .collect();
    let full = std::format!("`probe.two`: {}", SettingError::Full);
    assert!(about.iter().all(|w| w.message == full), "{about:?}");
    assert_eq!(about.len(), 1, "{warnings:?}");
}

#[test]
fn the_room_made_for_a_plugin_s_settings_is_enough_to_define_them_all() {
    use paperpaw_sdk::builtin::groups::OTHER;
    let (_shown, described) = probe_described();
    let own = [
        paperpaw_sdk::SettingSpec::switch("probe.own", "probe.own.title", true, OTHER),
        paperpaw_sdk::SettingSpec::switch("probe.other", "probe.own.title", true, OTHER),
    ];
    let mut settings = Settings::new();
    settings.check_described(&described).unwrap();
    settings.try_reserve_with_fixed(own.len(), 0, 0).unwrap();
    // The room for the described ones is next to that for the plugin's
    // other settings: asked for after theirs, it is not instead of it.
    settings
        .try_reserve_described(described.len(), own.len())
        .unwrap();
    FAIL_NTH.with(|fail| fail.set(1));
    let defined = own
        .into_iter()
        .try_for_each(|spec| settings.define(spec))
        .and_then(|()| settings.define_described(described));
    assert_eq!(
        FAIL_NTH.with(|fail| fail.replace(0)),
        1,
        "more room asked for"
    );
    defined.unwrap();
    assert_eq!(ids(&settings).len(), 5);
}

#[test]
fn whether_a_described_setting_is_a_secret_is_told_without_its_spec() {
    use crate::abi7::settings_visibility::SettingIdentity;
    use paperpaw_sdk::builtin::groups::OTHER;
    static SECRET: paperpaw_sdk::SettingExtra = paperpaw_sdk::SettingExtra::new().secret();
    let (_shown, mut settings) = probe();
    let key = paperpaw_sdk::SettingSpec::text("probe.key", "Key", "", OTHER);
    settings.define(key.with_extra(&SECRET)).unwrap();
    let secret = |key: &str| SettingIdentity::new(key).unwrap().secret(&settings);
    // Asked of a placement too (`probe@2.one` is `probe.one` placed).
    FAIL_NTH.with(|fail| fail.set(1));
    assert_eq!(secret("probe.one"), Some(false));
    assert_eq!(secret("probe@2.one"), Some(false));
    assert_eq!(secret("probe.key"), Some(true));
    assert_eq!(secret("probe@2.key"), Some(true));
    assert_eq!(secret("probe.none"), None);
    assert_eq!(secret("probe@2.none"), None);
    assert_eq!(
        FAIL_NTH.with(|fail| fail.replace(0)),
        1,
        "the heap was asked"
    );
    assert!(!settings.any_made());
}

/// An empty text as a setting's default on the wire.
const EMPTY: &[u8] = &[1, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0];

/// 8 as a setting's default on the wire.
const EIGHT: &[u8] = &[1, 0, 0, 0, 2, 0, 0, 0, 8, 0, 0, 0, 8, 0, 0, 0, 0, 0, 0, 0];

/// A number from 0 to 9 that is 8 until it is set.
fn number(name: &str, flags: u32) -> Record<'_> {
    Record::SettingSpec(wire::SettingSpec {
        name,
        kind: 1,
        flags,
        group: "probe.group",
        title: "Number",
        help: "",
        default: EIGHT,
        min: 0,
        max: 9,
        choices: "",
    })
}

/// A kernel with `probe`'s settings `lock` (a guarded switch), `open` (a
/// switch), `name` (a text), `most` (a guarded number) and `some` (a number).
fn locked_kernel() -> (Rc<Shown>, crate::Kernel) {
    let mut kernel = crate::Kernel::builder(crate::tests::DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    let name = Record::SettingSpec(wire::SettingSpec {
        name: "name",
        kind: 3,
        flags: 0,
        group: "probe.group",
        title: "Name",
        help: "",
        default: EMPTY,
        min: 0,
        max: 0,
        choices: "",
    });
    let records = [
        switch("lock", "Lock", 2),
        switch("open", "Open", 0),
        name,
        number("most", 2),
        number("some", 0),
    ];
    let shown = define_plugin(&mut kernel.shared.settings, "probe", &records);
    (shown, kernel)
}

/// Whether the armed lack of room ([`no_room_for_a_spec`]) was met; it is
/// taken back.
fn met_no_room() -> bool {
    FAIL_SIZE.with(|size| size.replace(0)) == 0
}

#[test]
fn a_guarded_setting_stays_another_plugin_s_to_keep_without_room_for_its_spec() {
    use paperpaw_sdk::{Command, Id};
    let (_shown, kernel) = locked_kernel();
    let settings = &kernel.shared.settings;
    let reserved = crate::commands::Reserved::of(settings, &kernel.shared.registry);
    let off = |id| Command::SetSetting(Id::new_static(id), SettingValue::Bool(false));
    let mut out = std::vec![
        off("probe.lock"),
        off("probe.open"),
        Command::ResetSetting(Id::new_static("probe.lock")),
    ];
    no_room_for_a_spec();
    crate::commands::vet_reserved(&mut out, 0, "other", &reserved);
    // Told from what is kept of it: no spec was asked for.
    assert!(!met_no_room());
    assert_eq!(out, [off("probe.open")]);
    assert!(settings.defines_guarded("probe.lock") && !settings.defines_guarded("probe.open"));
    assert!(!settings.any_made());
}

#[test]
fn a_setting_set_without_room_for_its_spec_is_not_called_unknown() {
    use paperpaw_sdk::Instant;
    let (_shown, mut kernel) = locked_kernel();
    no_room_for_a_spec();
    let refused = kernel.set_setting(Instant(10), "probe.open", "off");
    assert!(met_no_room());
    assert_eq!(refused.err(), Some("no room to read this setting now"));
    assert_eq!(
        kernel.set_setting(Instant(20), "probe.none", "off").err(),
        Some("no such setting")
    );
    assert!(kernel.set_setting(Instant(30), "probe.open", "off").is_ok());
    assert_eq!(
        kernel.shared.settings.get("probe.open"),
        Some(&SettingValue::Bool(false))
    );
}

#[test]
fn text_typed_for_a_setting_is_set_without_a_spec_kept_for_it() {
    use paperpaw_sdk::builtin::{events, params};
    use paperpaw_sdk::event::EventData;
    use paperpaw_sdk::{Instant, Params, Typed};
    let typed = Typed::new("Ann", Params::new().with(params::ID, "probe.name")).to_params();
    let (_shown, mut kernel) = locked_kernel();
    // No room for a spec to keep: asked whether the setting is there
    // without one, what was typed is not dropped unsaid.
    no_room_for_a_spec();
    let _ = kernel.send_event(Instant(10), events::SETTING_TYPED.id().clone(), typed);
    assert!(!met_no_room());
    assert_eq!(kernel.shared.settings.text("probe.name"), "Ann");
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn a_spec_left_out_for_lack_of_room_is_said_at_the_end_of_the_round() {
    let (_shown, mut kernel) = locked_kernel();
    let _ = kernel.finish();
    assert!(!warned(&mut kernel, "no room to read a setting"));
    // Who asks so cannot tell it from a setting that is not there.
    no_room_for_a_spec();
    assert!(kernel.shared.settings.spec("probe.open").is_none());
    assert!(met_no_room());
    assert!(kernel.shared.settings.went_short());
    let _ = kernel.finish();
    assert!(warned(&mut kernel, "no room to read a setting"));
    // Said once: the next round knows nothing of it.
    assert!(!kernel.shared.settings.went_short());
    let _ = kernel.finish();
    assert!(!warned(&mut kernel, "no room to read a setting"));
}

#[test]
fn no_mode_limits_a_guarded_setting_for_lack_of_room_for_its_spec() {
    use paperpaw_sdk::{Command, Id, power::Wear, presets::Preset};
    let (_shown, mut kernel) = locked_kernel();
    let low = Id::new_static("probe.low");
    let limits = "power.level = saving\nprobe.most <= 3\nprobe.some <= 3";
    let mode = Preset::new(
        low.clone(),
        paperpaw_sdk::power::MODE_KIND,
        "Low",
        "",
        limits,
    );
    kernel.shared.settings.presets_mut().add(mode);
    no_room_for_a_spec();
    kernel.apply(Command::Wear(Wear::new(low)));
    // Whether one is guarded is told from what is kept of it.
    assert!(!met_no_room());
    let settings = &kernel.shared.settings;
    assert_eq!(
        (settings.int("probe.most"), settings.int("probe.some")),
        (8, 3)
    );
}

#[test]
fn a_guarded_setting_takes_no_other_plugin_s_default_for_lack_of_room() {
    use paperpaw_sdk::Id;
    let (_shown, mut kernel) = locked_kernel();
    let (other, lock) = (Id::new_static("other"), Id::new_static("probe.lock"));
    kernel.plugin_defaults.push((other, lock, "off".into()));
    no_room_for_a_spec();
    kernel.apply_defaults(&[]);
    assert!(!met_no_room());
    assert_eq!(
        kernel.shared.settings.get("probe.lock"),
        Some(&SettingValue::Bool(true))
    );
    assert!(warned(&mut kernel, "it keeps the device safe"));
}
