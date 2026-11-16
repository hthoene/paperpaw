//! Settings written then read back: any value of any kind a plugin's
//! setting takes is saved to `paperpaw.ini` (or a book's own settings
//! file) so that the next start reads the same value (ADR 0006); values
//! reset are gone from the file; a broken book settings file leaves the
//! global values.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]
// Tests panic on purpose when something is wrong.

mod common;

use std::borrow::Cow;

use common::{DEVICE, Harness};
use paperpaw_core::Items;
use paperpaw_core::Kernel;
use paperpaw_core::sdk::builtin::CONFIG_FILE;
use paperpaw_core::sdk::{
    Command, Manifest, MemoryStorage, RegisterError, Setting, SettingSpec, SettingValue, Text,
};
use paperpaw_platform::Instant;
use proptest::prelude::*;

const FLAG: Setting<bool> = Setting::new("rt.flag");
const COUNT: Setting<i64> = Setting::new("rt.count");
const MODE: Setting<Text> = Setting::new("rt.mode");
const NAME: Setting<Text> = Setting::new("rt.name");
const LIST: Setting<Text> = Setting::new("rt.list");
const PAGE: Setting<i64> = Setting::new("rt.page");
const FACE: Setting<Text> = Setting::new("rt.face");
const MODES: [&str; 3] = ["rt.mode.a", "rt.mode.b", "rt.mode.c"];
const MIN: i64 = -1_000_000_000_000;
const MAX: i64 = 1_000_000_000_000;

/// A plugin with a setting of each kind, two of them per book.
fn settings() -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    |r| {
        r.add_setting(SettingSpec::switch(
            FLAG.static_str(),
            "rt.flag",
            false,
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(SettingSpec::number(
            COUNT.static_str(),
            "rt.count",
            MIN,
            MAX,
            0,
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(SettingSpec::choice(
            MODE.static_str(),
            "rt.mode",
            &MODES,
            MODES[0],
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(SettingSpec::text(
            NAME.static_str(),
            "rt.name",
            "nobody",
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(SettingSpec::list(
            LIST.static_str(),
            "rt.list",
            "",
            paperpaw_sdk::builtin::groups::OTHER,
        ))?;
        r.add_setting(
            SettingSpec::number(
                PAGE.static_str(),
                "rt.page",
                1,
                100,
                10,
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .scoped(),
        )?;
        r.add_setting(
            SettingSpec::text(
                FACE.static_str(),
                "rt.face",
                "plain",
                paperpaw_sdk::builtin::groups::OTHER,
            )
            .scoped(),
        )?;
        Ok(())
    }
}

/// What [`settings`] registers under.
fn settings_manifest() -> Manifest {
    common::manifest("rt")
}

fn kernel(harness: &Harness, storage: MemoryStorage) -> Kernel {
    let mut kernel = Kernel::builder(DEVICE)
        .plugin(harness.plugin())
        .items(settings_manifest(), settings())
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    common::render(&mut kernel);
    kernel
}

/// A storage with what `kernel` saved at `path`.
fn saved(kernel: &Kernel, path: &str) -> MemoryStorage {
    let data = paperpaw_core::sdk::read_file(kernel.storage(), path, 1 << 20).unwrap_or_default();
    MemoryStorage::new().with(path, data)
}

fn value(kernel: &Kernel, id: &str) -> Option<SettingValue> {
    kernel.settings().get(id).cloned()
}

/// A setting and a value for it, as text the way files write it.
#[derive(Debug, Clone)]
enum Change {
    Flag(bool),
    Count(i64),
    Mode(usize),
    Name(String),
    List(Vec<String>),
}

impl Change {
    fn id(&self) -> &'static str {
        match self {
            Change::Flag(_) => FLAG.static_str(),
            Change::Count(_) => COUNT.static_str(),
            Change::Mode(_) => MODE.static_str(),
            Change::Name(_) => NAME.static_str(),
            Change::List(_) => LIST.static_str(),
        }
    }

    fn text(&self) -> String {
        match self {
            Change::Flag(on) => String::from(if *on { "on" } else { "off" }),
            Change::Count(n) => n.to_string(),
            Change::Mode(i) => String::from(MODES[*i]),
            Change::Name(text) => text.clone(),
            Change::List(ids) => ids.join(","),
        }
    }
}

/// Text as people type it, with what files treat specially: quotes,
/// backslashes, `=`, `#`, `;`, brackets, spaces and tabs at either end,
/// new lines, characters beyond ASCII. Carriage returns are left out:
/// files drop them (see `paperpaw_config::quote`).
fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        "[ -~]{0,24}",
        "[ \t\n\"\\\\=#;\\[\\]a-zäöü€\u{1F600}\u{a0}\u{2028}]{0,24}",
        any::<String>().prop_map(|s| s.replace('\r', "")),
    ]
}

fn change() -> impl Strategy<Value = Change> {
    prop_oneof![
        any::<bool>().prop_map(Change::Flag),
        (MIN..=MAX).prop_map(Change::Count),
        prop_oneof![Just(MIN), Just(MAX), Just(0), Just(-1)].prop_map(Change::Count),
        (0..MODES.len()).prop_map(Change::Mode),
        text().prop_map(Change::Name),
        prop::collection::vec("[a-z]{1,6}\\.[a-z]{1,6}", 0..4).prop_map(Change::List),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(128)) })]

    /// Every setting changed is read back the same at the next start.
    #[test]
    fn settings_saved_are_read_back_the_same(changes in prop::collection::vec(change(), 1..8)) {
        let harness = Harness::new();
        let mut first = kernel(&harness, MemoryStorage::new());
        for (i, change) in changes.iter().enumerate() {
            let result = first.set_setting(Instant(10 + i as u64), change.id(), &change.text());
            prop_assert!(result.is_ok(), "{change:?}: {result:?}");
        }
        let ids = [FLAG.static_str(), COUNT.static_str(), MODE.static_str(), NAME.static_str(), LIST.static_str()];
        let before: Vec<_> = ids.iter().map(|id| value(&first, id)).collect();
        let mut second = kernel(&harness, saved(&first, CONFIG_FILE));
        let after: Vec<_> = ids.iter().map(|id| value(&second, id)).collect();
        prop_assert_eq!(&before, &after);
        let warnings = second.take_warnings();
        prop_assert!(warnings.is_empty(), "{:?}", warnings);
    }

    /// A book's own settings are read back the same when the book opens
    /// again.
    #[test]
    fn book_settings_saved_are_read_back_the_same(page in 1i64..=100, face in text()) {
        let path = "books/a/settings.ini";
        let harness = Harness::new();
        let mut first = kernel(&harness, MemoryStorage::new());
        harness.run(&mut first, 10, [Command::SetScope(Some(String::from(path)))]);
        first.set_setting(Instant(20), "core.scope-only", "on").unwrap();
        first.set_setting(Instant(30), PAGE.static_str(), &page.to_string()).unwrap();
        first.set_setting(Instant(40), FACE.static_str(), &face).unwrap();
        prop_assert_eq!(value(&first, PAGE.static_str()), Some(SettingValue::Int(page)));

        let mut second = kernel(&harness, saved(&first, path));
        prop_assert_eq!(value(&second, PAGE.static_str()), Some(SettingValue::Int(10)));
        harness.run(&mut second, 10, [Command::SetScope(Some(String::from(path)))]);
        prop_assert_eq!(value(&second, PAGE.static_str()), Some(SettingValue::Int(page)));
        prop_assert_eq!(
            value(&second, FACE.static_str()),
            Some(SettingValue::Text(Cow::Owned(face)))
        );
    }
}

#[test]
fn settings_reset_are_gone_from_the_file() {
    let harness = Harness::new();
    let mut first = kernel(&harness, MemoryStorage::new());
    first
        .set_setting(Instant(10), COUNT.static_str(), "42")
        .unwrap();
    first
        .set_setting(Instant(20), NAME.static_str(), "kept")
        .unwrap();
    harness.run(&mut first, 30, [Command::ResetSetting(COUNT.id().clone())]);
    let second = kernel(&harness, saved(&first, CONFIG_FILE));
    assert_eq!(
        value(&second, COUNT.static_str()),
        Some(SettingValue::Int(0))
    );
    assert_eq!(
        value(&second, NAME.static_str()),
        Some(SettingValue::Text(Cow::Borrowed("kept")))
    );
    let text = paperpaw_core::sdk::read_file(first.storage(), CONFIG_FILE, 4096).unwrap();
    assert!(!String::from_utf8(text).unwrap().contains("count"));
}

#[test]
fn values_a_setting_does_not_take_are_refused_and_not_saved() {
    let harness = Harness::new();
    let mut kernel = kernel(&harness, MemoryStorage::new());
    for (id, text) in [
        (COUNT.static_str(), "1000000000001"),
        (COUNT.static_str(), "12abc"),
        (COUNT.static_str(), ""),
        (FLAG.static_str(), "perhaps"),
        (MODE.static_str(), "rt.mode.z"),
        ("rt.unknown", "1"),
        ("not an id", "1"),
        ("", ""),
    ] {
        assert!(
            kernel.set_setting(Instant(10), id, text).is_err(),
            "{id} = {text}"
        );
    }
    assert!(!kernel.storage().exists(CONFIG_FILE));
    // Through a command, the same: a warning, nothing saved.
    harness.run(
        &mut kernel,
        20,
        [Command::SetSetting(
            COUNT.id().clone(),
            SettingValue::Text(Cow::Borrowed("x")),
        )],
    );
    assert!(!kernel.storage().exists(CONFIG_FILE));
    assert_eq!(
        value(&kernel, COUNT.static_str()),
        Some(SettingValue::Int(0))
    );
    assert!(!kernel.take_warnings().is_empty());
}

#[test]
fn a_broken_book_settings_file_leaves_the_global_values() {
    let path = "books/b/settings.ini";
    let files: [&[u8]; 5] = [
        b"[rt]\npage = 500\nface = \"unterminated\n",
        b"[core]\nscope-only = on\n[rt]\nflag = on\ncount = 5\npage = -1\n",
        b"\xFF\xFE[rt]\npage = 20\n",
        b"[rt\npage = 20\n=\n[[[\n",
        b"",
    ];
    for file in files {
        let harness = Harness::new();
        let storage = MemoryStorage::new().with(path, file.to_vec());
        let mut kernel = kernel(&harness, storage);
        harness.run(
            &mut kernel,
            10,
            [Command::SetScope(Some(String::from(path)))],
        );
        let shown = String::from_utf8_lossy(file);
        assert_eq!(
            value(&kernel, PAGE.static_str()),
            Some(SettingValue::Int(10)),
            "{shown}"
        );
        // Settings not kept per book are never taken from such a file.
        assert_eq!(
            value(&kernel, FLAG.static_str()),
            Some(SettingValue::Bool(false)),
            "{shown}"
        );
        assert_eq!(
            value(&kernel, COUNT.static_str()),
            Some(SettingValue::Int(0)),
            "{shown}"
        );
    }
}

#[test]
fn a_book_scope_ends_with_the_screen_that_set_it() {
    let path = "books/c/settings.ini";
    let storage =
        MemoryStorage::new().with(path, b"[core]\nscope-only = on\n[rt]\npage = 33\n".to_vec());
    let harness = Harness::new();
    let mut kernel = kernel(&harness, storage);
    harness.run(
        &mut kernel,
        10,
        [Command::OpenScreen(
            common::FULL,
            paperpaw_core::sdk::Params::new(),
        )],
    );
    harness.run(
        &mut kernel,
        20,
        [Command::SetScope(Some(String::from(path)))],
    );
    assert_eq!(
        value(&kernel, PAGE.static_str()),
        Some(SettingValue::Int(33))
    );
    harness.run(&mut kernel, 30, [Command::Back]);
    assert_eq!(kernel.active_screen(), Some(&common::BASE));
    assert_eq!(
        value(&kernel, PAGE.static_str()),
        Some(SettingValue::Int(10))
    );
    // A change now is global, not the closed book's.
    kernel
        .set_setting(Instant(40), PAGE.static_str(), "44")
        .unwrap();
    let book = paperpaw_core::sdk::read_file(kernel.storage(), path, 4096).unwrap();
    assert!(String::from_utf8(book).unwrap().contains("page = 33"));
}

#[test]
fn ids_of_settings_are_not_confused_across_sections() {
    // `[rt] count` and `[rtx] count` are different settings; a key with
    // a dot stays in its section.
    let harness = Harness::new();
    let storage = MemoryStorage::new().with(
        CONFIG_FILE,
        b"[rtx]\ncount = 5\n[rt.count]\nx = 1\n[rt]\ncount = 7\n".to_vec(),
    );
    let mut kernel = kernel(&harness, storage);
    assert_eq!(
        value(&kernel, COUNT.static_str()),
        Some(SettingValue::Int(7))
    );
    kernel
        .set_setting(Instant(10), COUNT.static_str(), "8")
        .unwrap();
    let text = String::from_utf8(
        paperpaw_core::sdk::read_file(kernel.storage(), CONFIG_FILE, 4096).unwrap(),
    )
    .unwrap();
    assert!(text.contains("[rtx]\ncount = 5"), "{text}");
    assert!(text.contains("[rt]\ncount = 8"), "{text}");
}
