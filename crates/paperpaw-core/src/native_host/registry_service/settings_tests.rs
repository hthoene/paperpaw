//! `core.registry` ops 1-3 and 16-18 against `registry.md` 2 and 3: framing,
//! paging, scopes, what a value shows to whom, presets and placements.
#![allow(clippy::unwrap_used, reason = "fixture failures must fail tests")]
extern crate std;

use super::*;
use crate::abi7::{services_wire::service_schema, settings_wire::settings_decode};
use crate::native_host::host_services::{Caller, HostServices};
use alloc::{borrow::Cow, boxed::Box, format, string::String, vec::Vec};
use paperpaw_i18n::Translator;
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_SPACE, ERR_NOT_FOUND};
use paperpaw_platform::Instant;
use paperpaw_sdk::{
    Canvas, Cx, Env, Handled, Id, Manifest, Plugin, Priority, RegisterError, Registrar, Screen,
    ScreenSpec, SettingExtra, SettingGroupSpec, SettingSpec, SettingValue, Settings, Version,
    builtin::{groups::OTHER, slots::SETTINGS as SETTINGS_SLOT},
    presets::Preset,
};

struct Quiet;
impl Screen for Quiet {
    fn draw(&mut self, _: &mut Canvas<'_>, _: &Env<'_>) {}
    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

fn id(text: &str) -> Id {
    Id::parse(text).unwrap()
}

static MODES: [&str; 2] = ["cfg.mode.a", "cfg.mode.b"];
static SECRET: SettingExtra = SettingExtra::new().secret();
static FULL: SettingExtra = SettingExtra::new()
    .unit("cfg.unit.pt")
    .step(2)
    .section("cfg.section.look")
    .shown_if(Id::new_static("cfg.flag"), "on")
    .follows(Id::new_static("cfg.other"));
static PLUGIN_SECTION: SettingExtra = SettingExtra::new().section_of_plugin();
static PRESETS: [Preset; 3] = [
    Preset::fixed(
        "cfg.night",
        "cfg.power",
        "cfg.preset.night",
        "cfg.preset.night.help",
        "cfg.size = 20\n# a comment\ncfg.nothing = 1\ncfg.size = 22\ncfg.mode = cfg.mode.b\ncfg.size = bad\n",
    ),
    Preset::fixed("cfg.day", "cfg.power", "Day", "", "cfg.flag = on\n"),
    Preset::fixed(
        "cfg.dark",
        "cfg.theme",
        "Dark",
        "",
        "core.grant.style.cfg = on\ncfg.key = abc\ncfg.flag = off\n",
    ),
];

/// A plugin with settings of every shape, a group and presets.
struct Cfg;
impl Plugin for Cfg {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("cfg"), "Cfg", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_setting_group(SettingGroupSpec::new(id("cfg.top"), "cfg.group.top").order(1))?;
        let top = id("cfg.top");
        r.add_setting(SettingSpec::switch(
            "cfg.flag",
            "cfg.title.flag",
            false,
            top.clone(),
        ))?;
        r.add_setting(
            SettingSpec::number("cfg.size", "cfg.title.size", 8, 40, 12, top.clone())
                .with_extra(&FULL)
                .description("cfg.help.size"),
        )?;
        r.add_setting(SettingSpec::choice(
            "cfg.mode",
            "cfg.title.mode",
            &MODES,
            "cfg.mode.a",
            top.clone(),
        ))?;
        r.add_setting(SettingSpec::text(
            "cfg.name",
            "cfg.title.name",
            "x",
            id("cfg.nowhere"),
        ))?;
        r.add_setting(
            SettingSpec::text("cfg.key", "cfg.title.key", "", top.clone()).with_extra(&SECRET),
        )?;
        r.add_setting(
            SettingSpec::switch("cfg.guard", "cfg.title.guard", false, top.clone())
                .guarded()
                .advanced()
                .scoped(),
        )?;
        r.add_setting(
            SettingSpec::text("cfg.plain", "cfg.title.plain", "", top.clone())
                .with_extra(&PLUGIN_SECTION)
                .kept_on_reset(),
        )?;
        r.add_setting(SettingSpec::text(
            "cfg.long",
            "cfg.title.long",
            "0123456789012345678901234567890123456789012345678901234567890123456789",
            top.clone(),
        ))?;
        r.add_setting(SettingSpec::text(
            "cfg.clock.face",
            "cfg.title.face",
            "a",
            top.clone(),
        ))?;
        r.add_setting(SettingSpec::number(
            "cfg.clock.size",
            "cfg.title.csize",
            1,
            9,
            3,
            top,
        ))?;
        r.add_presets(&PRESETS)?;
        Ok(())
    }
}

/// Adds a choice to `cfg.mode`, and is the settings editor.
struct Ed;
impl Plugin for Ed {
    fn manifest(&self) -> Manifest {
        Manifest::new(Id::new_static("ed"), "Ed", Version::new(1, 0, 0))
            .uses(paperpaw_sdk::Uses::NOTHING)
    }
    fn register(&self, r: &mut Registrar) -> Result<(), RegisterError> {
        r.add_screen(ScreenSpec::new(
            id("ed.main"),
            "ed.screen.main",
            Box::new(|_, _| Box::new(Quiet)),
        ))?;
        r.bind_slot(SETTINGS_SLOT, id("ed.main"), Priority::DEFAULT)?;
        r.add_choice(id("cfg.mode"), "ed.mode.c")?;
        Ok(())
    }
}

fn kernel() -> crate::Kernel {
    crate::Kernel::builder(crate::tests::DEVICE)
        .plugin(Cfg)
        .plugin(Ed)
        .build()
        .unwrap()
}

struct View<'a> {
    env: &'a Env<'a>,
    sources: Sources<'a>,
    hosts: HostServices,
}
impl Frame for View<'_> {
    fn settings(&self) -> &Settings {
        self.env.settings()
    }
    fn translator(&self) -> &Translator {
        self.env.i18n()
    }
    fn registry(&self) -> Result<&dyn RegistryView, i32> {
        Ok(&self.sources)
    }
}

fn op(id: u32) -> &'static ServiceOp {
    service_schema(schema::KEY, schema::MAJOR)
        .unwrap()
        .op(id)
        .unwrap()
}

fn call_as(
    kernel: &crate::Kernel,
    plugin: &str,
    id: u32,
    input: &[u8],
    room: usize,
) -> Result<Vec<u8>, i32> {
    let env = kernel.shared.env(Instant(0));
    let view = View {
        env: &env,
        sources: Sources {
            env: &env,
            directory: None,
            generation: &kernel.shared.registry_generation,
        },
        hosts: HostServices::new(),
    };
    let caller = Caller {
        owner: 7,
        plugin,
        features: crate::native_host::admitted_features::AdmittedFeatures::NONE,
        hosts: &view.hosts,
    };
    let mut out = std::vec![0xaa; room];
    let used = backend(&view, &caller, op(id), input, &mut out)?;
    out.truncate(used);
    Ok(out)
}

fn counted(record: Record<'_>) -> Vec<u8> {
    let mut input = std::vec![0; 512];
    let used = wire::records_encode_counted(&[record], 1, 256, &mut input).unwrap();
    input.truncate(used);
    input
}

fn query(scope: &str, cursor: u32, generation: u32, flags: u32) -> Vec<u8> {
    counted(Record::ServiceRegQuery(wire::ServiceRegQuery {
        scope,
        cursor,
        generation,
        flags,
    }))
}

fn key(key: &str) -> Vec<u8> {
    counted(Record::ServiceRegKey(wire::ServiceRegKey { key }))
}

fn value_text(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::from("-");
    }
    format!("{:?}", settings_decode(bytes).unwrap())
}

/// A reply as one text row per record, and its `end`.
fn read(reply: &[u8]) -> (Vec<String>, Option<wire::ServiceEnd>) {
    let mut rows = Vec::new();
    let mut end = None;
    let mut rest = reply;
    while !rest.is_empty() {
        let (record, used) = wire::record_decode_prefix(rest).unwrap();
        rest = &rest[used..];
        assert!(end.is_none(), "nothing follows the end");
        rows.push(match record {
            Record::ServiceEnd(e) => {
                end = Some(e);
                continue;
            }
            Record::ServiceRegSetting(r) => format!(
                "setting {} group={} kind={} flags={} source={} title={} value={}",
                r.key,
                r.group,
                r.kind,
                r.flags,
                r.source,
                r.title,
                value_text(r.value)
            ),
            Record::ServiceRegSettingExtra(r) => format!(
                "extra {}..{}/{} unit={} section={} follows={} shown={}={} editor={}:{} choices={}",
                r.min,
                r.max,
                r.step,
                r.unit,
                r.section,
                r.follows,
                r.shown_id,
                r.shown_value,
                r.editor_kind,
                r.editor,
                r.choices
            ),
            Record::ServiceRegChoice(r) => format!("choice {} {}", r.value, r.title),
            Record::ServiceRegItem(r) => {
                format!("item {} {} {} {}", r.id, r.plugin, r.flags, r.title)
            }
            Record::ServiceRegValue(r) => {
                format!("value {} {} {}", r.key, value_text(r.value), r.flags)
            }
            Record::ServiceRegText(r) => format!("text {}", r.text),
            other => panic!("unexpected {other:?}"),
        });
    }
    (rows, end)
}

fn page_as(
    kernel: &crate::Kernel,
    plugin: &str,
    id: u32,
    scope: &str,
    flags: u32,
) -> (Vec<String>, wire::ServiceEnd) {
    let reply = call_as(kernel, plugin, id, &query(scope, 0, 0, flags), 4096).unwrap();
    let (rows, end) = read(&reply);
    (rows, end.unwrap())
}

fn page(kernel: &crate::Kernel, id: u32, scope: &str) -> (Vec<String>, wire::ServiceEnd) {
    page_as(kernel, "reader", id, scope, 0)
}

fn pages(kernel: &crate::Kernel, id: u32, scope: &str) -> Vec<String> {
    let mut all = Vec::new();
    let (mut cursor, mut generation) = (0, 0);
    loop {
        let reply = call_as(
            kernel,
            "reader",
            id,
            &query(scope, cursor, generation, 0),
            4096,
        )
        .unwrap();
        let (rows, end) = read(&reply);
        let end = end.unwrap();
        all.extend(rows);
        if end.flags & 1 != 0 {
            return all;
        }
        (cursor, generation) = (end.next, end.generation);
    }
}

fn detail_as(kernel: &crate::Kernel, plugin: &str, id: u32, k: &str) -> Result<Vec<String>, i32> {
    let reply = call_as(kernel, plugin, id, &key(k), 4096)?;
    let (rows, end) = read(&reply);
    assert!(end.is_none(), "a detail op has no end");
    Ok(rows)
}

/// The rows of the `cfg.*` settings by key.
fn row<'a>(rows: &'a [String], key: &str) -> &'a str {
    let prefix = format!("setting {key} ");
    rows.iter().find(|r| r.starts_with(&prefix)).unwrap()
}

fn flags_of(row: &str) -> u32 {
    row.split(" flags=")
        .nth(1)
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap()
}

#[test]
fn settings_list_rows_in_registry_order_with_kind_group_and_title() {
    let kernel = kernel();
    let (rows, end) = page(&kernel, schema::OP_SETTINGS, "cfg");
    assert_eq!((end.flags, end.total), (1, 10));
    let keys: Vec<&str> = rows.iter().map(|r| r.split(' ').nth(1).unwrap()).collect();
    assert_eq!(
        keys,
        [
            "cfg.flag",
            "cfg.size",
            "cfg.mode",
            "cfg.name",
            "cfg.key",
            "cfg.guard",
            "cfg.plain",
            "cfg.long",
            "cfg.clock.face",
            "cfg.clock.size"
        ]
    );
    // Shown (512) in its group, a switch at its default; the title is the key
    // where no catalog has it; the value is `settings_value` encoded.
    assert_eq!(
        row(&rows, "cfg.flag"),
        "setting cfg.flag group=cfg.top kind=0 flags=1600 source=0 title=cfg.title.flag value=Bool(false)"
    );
    assert!(row(&rows, "cfg.size").contains("kind=1"));
    assert!(row(&rows, "cfg.size").ends_with("value=Int(12)"));
    assert!(row(&rows, "cfg.mode").contains("kind=2"));
    assert!(row(&rows, "cfg.mode").ends_with("value=Text(\"cfg.mode.a\")"));
    // A group nothing registered reads as the catch-all group.
    assert!(row(&rows, "cfg.name").contains(&format!("group={}", OTHER.as_str())));
}

#[test]
fn scopes_are_a_group_a_plugin_or_nothing_and_an_unknown_one_is_empty() {
    let kernel = kernel();
    let all = pages(&kernel, schema::OP_SETTINGS, "");
    assert!(all.len() > 10);
    let (rows, end) = page(&kernel, schema::OP_SETTINGS, "cfg.top");
    assert_eq!((rows.len(), end.total), (9, 9), "cfg.name sits in no group");
    assert!(rows.iter().all(|r| r.contains("group=cfg.top")));
    let (rows, _) = page(&kernel, schema::OP_SETTINGS, OTHER.as_str());
    assert!(rows.iter().any(|r| r.starts_with("setting cfg.name ")));
    let (rows, end) = page(&kernel, schema::OP_SETTINGS, "nobody");
    assert!(rows.is_empty());
    assert_eq!((end.flags, end.total, end.next), (1, 0, 0));
    // Choices of no setting, no choice setting and a placement id: empty too.
    for scope in ["", "nobody.x", "cfg.size", "cfg.clock@2.face"] {
        let (rows, end) = page(&kernel, schema::OP_CHOICES, scope);
        assert!(rows.is_empty() && end.total == 0, "{scope}");
    }
}

#[test]
fn a_long_list_pages_whole_rows_in_one_generation() {
    let mut kernel = kernel();
    for index in 0..80 {
        let spec = SettingSpec::number(
            Box::leak(format!("many.n{index:02}").into_boxed_str()),
            "many.title",
            0,
            9,
            1,
            OTHER,
        );
        kernel.shared.settings.define(spec).unwrap();
    }
    let (first, end) = page(&kernel, schema::OP_SETTINGS, "many");
    assert_eq!((first.len(), end.total, end.flags), (32, 80, 0));
    let all = pages(&kernel, schema::OP_SETTINGS, "many");
    assert_eq!(all.len(), 80);
    assert!(all[79].starts_with("setting many.n79 "));
    // A page of another generation, and one past the total, are refused.
    let stale = query("many", 32, end.generation + 1, 0);
    assert_eq!(
        call_as(&kernel, "reader", schema::OP_SETTINGS, &stale, 4096),
        Err(ERR_STATE)
    );
    let past = query("many", 81, end.generation, 0);
    assert_eq!(
        call_as(&kernel, "reader", schema::OP_SETTINGS, &past, 4096),
        Err(ERR_INVALID)
    );
    // A buffer that cannot hold one row and the end.
    assert_eq!(
        call_as(
            &kernel,
            "reader",
            schema::OP_SETTINGS,
            &query("many", 0, 0, 0),
            40
        ),
        Err(ERR_NO_SPACE)
    );
}

#[test]
fn flags_say_what_the_setting_is_and_where_its_value_comes_from() {
    let mut kernel = kernel();
    {
        let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
        // Guarded 1, scoped 4, advanced 16, has value 64, shown 512.
        assert_eq!(flags_of(row(&rows, "cfg.guard")), 1 | 4 | 16 | 64 | 512);
        // Kept on reset 8.
        assert_eq!(flags_of(row(&rows, "cfg.plain")), 8 | 64 | 512);
        // `cfg.flag` governs the showing of `cfg.size` (1024); `cfg.size`
        // shows only while `cfg.flag` is on.
        assert_eq!(flags_of(row(&rows, "cfg.flag")), 64 | 512 | 1024);
        assert_eq!(flags_of(row(&rows, "cfg.size")), 64);
        // A value over 64 bytes encoded: long (128), the value empty.
        let long = row(&rows, "cfg.long");
        assert_eq!(flags_of(long), 64 | 128 | 512);
        assert!(long.ends_with("value=-"));
    }
    let settings = &mut kernel.shared.settings;
    settings.set("cfg.flag", SettingValue::Bool(true)).unwrap();
    settings
        .set("cfg.name", SettingValue::Text("mine".into()))
        .unwrap();
    let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
    assert_eq!(flags_of(row(&rows, "cfg.size")), 64 | 512);
    assert!(row(&rows, "cfg.flag").contains("source=3"));
    assert!(row(&rows, "cfg.flag").ends_with("value=Bool(true)"));
    assert!(row(&rows, "cfg.name").contains("source=3"));
    // A power mode wears a value over the user's: a preset (1), and the
    // value shown is the worn one.
    let touched = kernel.shared.settings.wear(
        std::vec![(id("cfg.size"), SettingValue::Int(30))],
        Vec::new(),
    );
    assert_eq!(touched.len(), 1);
    let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
    assert!(row(&rows, "cfg.size").contains("source=1"));
    assert!(row(&rows, "cfg.size").ends_with("value=Int(30)"));
    // A value shown while it is chosen: preview (256), the kept one stays.
    kernel.shared.settings.set_preview(std::vec![(
        id("cfg.mode"),
        Some(SettingValue::Text("cfg.mode.b".into()))
    )]);
    let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
    assert_eq!(flags_of(row(&rows, "cfg.mode")) & 256, 256);
    assert!(row(&rows, "cfg.mode").ends_with("value=Text(\"cfg.mode.b\")"));
    assert_eq!(flags_of(row(&rows, "cfg.name")) & 256, 0);
    // A preview that equals the kept value is still in the preview set.
    let kept = kernel.shared.settings.get("cfg.name").cloned().unwrap();
    kernel
        .shared
        .settings
        .set_preview(std::vec![(id("cfg.name"), Some(kept))]);
    let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
    assert_eq!(flags_of(row(&rows, "cfg.name")) & 256, 256);
    assert_eq!(flags_of(row(&rows, "cfg.mode")) & 256, 0);
}

#[test]
fn every_rows_governs_flag_is_what_settings_governs_says() {
    let kernel = kernel();
    let settings = &kernel.shared.settings;
    // Found once for a list: sorted, each id once, the same answer per id.
    let ids = governing(settings).unwrap();
    assert!(ids.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(ids.contains(&"cfg.flag"));
    let rows = pages(&kernel, schema::OP_SETTINGS, "");
    let mut seen = 0;
    for row in rows.iter().filter(|row| row.starts_with("setting ")) {
        let key = row.split(' ').nth(1).unwrap();
        let flagged = flags_of(row) & GOVERNS != 0;
        assert_eq!(flagged, settings.governs(key), "{key}");
        assert_eq!(flagged, ids.binary_search(&key).is_ok(), "{key}");
        seen += 1;
    }
    assert_eq!(seen, settings.specs().count());
    // One setting alone asks `governs` itself: the same flag.
    let flag = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.flag").unwrap();
    assert_eq!(flags_of(&flag[0]) & GOVERNS, GOVERNS);
}

#[test]
fn secrets_grants_and_values_follow_slot_five() {
    let mut kernel = kernel();
    let settings = &mut kernel.shared.settings;
    settings
        .define(SettingSpec::switch(
            "core.grant.style.cfg",
            "grant",
            false,
            OTHER,
        ))
        .unwrap();
    settings
        .set("core.grant.style.cfg", SettingValue::Bool(true))
        .unwrap();
    settings
        .set("cfg.key", SettingValue::Text("hunter2".into()))
        .unwrap();
    for (plugin, grant_shown) in [("reader", false), ("cfg", false), ("ed", true)] {
        let (rows, _) = page_as(&kernel, plugin, schema::OP_SETTINGS, "cfg", 0);
        // A secret is never shown to anyone, not its own plugin: hidden (32)
        // and only whether it has a value (64).
        let secret = row(&rows, "cfg.key");
        assert_eq!(flags_of(secret) & (2 | 32 | 64), 2 | 32 | 64, "{plugin}");
        assert!(secret.ends_with("value=-"), "{secret}");
        // Where it comes from is withheld too: the default, not the user's.
        assert!(secret.contains("source=0"), "{plugin}: {secret}");
        // A public value is shown to every plugin.
        assert!(row(&rows, "cfg.flag").ends_with("value=Bool(false)"));
        let (rows, _) = page_as(&kernel, plugin, schema::OP_SETTINGS, "core.group.other", 0);
        let grant = row(&rows, "core.grant.style.cfg");
        if grant_shown {
            assert!(grant.ends_with("value=Bool(true)"), "{plugin}: {grant}");
            assert_eq!(flags_of(grant) & 32, 0);
        } else {
            // Hidden: withheld, and not even that it has one.
            assert!(grant.ends_with("value=-"), "{plugin}: {grant}");
            assert_eq!(flags_of(grant) & (32 | 64), 32, "{plugin}");
            assert!(grant.contains("source=0"), "{plugin}: {grant}");
        }
    }
    // An unset secret has no value.
    let mut kernel = self::kernel();
    kernel
        .shared
        .settings
        .define(SettingSpec::text("zz.key", "zz", "", OTHER).with_extra(&SECRET))
        .unwrap();
    let (rows, _) = page(&kernel, schema::OP_SETTINGS, "zz");
    assert_eq!(flags_of(row(&rows, "zz.key")) & (32 | 64), 32);
}

/// Whether a secret has a value is told by what the kernel keeps and by who
/// asks, never by which plugin the kernel's tracker last entered: a nested
/// call (a stage subscriber, a compose child) reads the same.
#[test]
fn the_secret_is_set_flag_does_not_follow_the_running_plugin() {
    let mut kernel = kernel();
    kernel
        .shared
        .settings
        .set("cfg.key", SettingValue::Text("hunter2".into()))
        .unwrap();
    let flags = |kernel: &crate::Kernel, plugin: &str| {
        let (rows, _) = page_as(kernel, plugin, schema::OP_SETTINGS, "cfg", 0);
        flags_of(row(&rows, "cfg.key")) & (32 | 64)
    };
    for plugin in ["reader", "cfg", "ed"] {
        let plain = flags(&kernel, plugin);
        assert_eq!(plain, 32 | 64, "{plugin}");
        // Whoever the tracker names, the asker sees the same.
        for running in ["cfg", "other", "ed"] {
            let _running = kernel.shared.meter.enter(running);
            assert_eq!(flags(&kernel, plugin), plain, "{plugin} under {running}");
        }
    }
}

#[test]
fn a_setting_is_three_records_and_an_unknown_key_is_not_found() {
    let kernel = kernel();
    let rows = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.size").unwrap();
    assert_eq!(rows.len(), 3);
    assert!(rows[0].starts_with("setting cfg.size group=cfg.top kind=1 "));
    assert_eq!(
        rows[1],
        "extra 8..40/2 unit=cfg.unit.pt section=cfg.section.look follows=cfg.other shown=cfg.flag=on editor=0: choices=0"
    );
    assert_eq!(rows[2], "text cfg.help.size");
    // No help: the two records.
    let rows = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.mode").unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows[1].ends_with("choices=3"), "{}", rows[1]);
    // A setting made for each plugin has its plugin's name as its section.
    let rows = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.plain").unwrap();
    assert!(rows[1].contains("section=plain.plugin.name"), "{}", rows[1]);
    for unknown in ["nobody.x", "", "cfg", "cfg.clock@2.face"] {
        assert_eq!(
            detail_as(&kernel, "reader", schema::OP_SETTING, unknown),
            Err(ERR_NOT_FOUND),
            "{unknown}"
        );
    }
    // The same authority as the list: a secret's value is not in the row.
    let rows = detail_as(&kernel, "cfg", schema::OP_SETTING, "cfg.key").unwrap();
    assert!(rows[0].contains("value=-"));
}

#[test]
fn titles_and_help_are_translated_and_cut_at_a_character() {
    let mut kernel = kernel();
    let long = "\u{e9}".repeat(600);
    let catalog = format!(
        "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
         cfg.title.flag = Schalter\ncfg.help.size = {long}\ncfg.title.mode = {long}\n\
         cfg.mode.b = Zweitens\n"
    );
    kernel
        .shared
        .translator
        .add(paperpaw_i18n::Catalog::parse(Cow::Owned(catalog)).unwrap());
    kernel.shared.translator.set_language("de");
    let rows = page(&kernel, schema::OP_SETTINGS, "cfg").0;
    assert!(row(&rows, "cfg.flag").contains("title=Schalter "));
    // 128 bytes: 64 two-byte characters.
    let mode = row(&rows, "cfg.mode");
    let title = mode
        .split(" title=")
        .nth(1)
        .unwrap()
        .split(" value=")
        .next()
        .unwrap();
    assert_eq!(title, "\u{e9}".repeat(64));
    let rows = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.size").unwrap();
    assert_eq!(rows[2], format!("text {}", "\u{e9}".repeat(512)));
    let (rows, _) = page(&kernel, schema::OP_CHOICES, "cfg.mode");
    assert_eq!(rows[1], "choice cfg.mode.b Zweitens");
}

#[test]
fn choices_are_the_settings_own_then_those_others_added() {
    let mut kernel = kernel();
    let (rows, end) = page(&kernel, schema::OP_CHOICES, "cfg.mode");
    assert_eq!(
        rows,
        [
            "choice cfg.mode.a cfg.mode.a",
            "choice cfg.mode.b cfg.mode.b",
            "choice ed.mode.c ed.mode.c"
        ]
    );
    assert_eq!((end.flags, end.total), (1, 3));
    // A value too long for its field cannot be a row: not counted either.
    let long: &'static str = Box::leak("x".repeat(70).into_boxed_str());
    assert!(kernel.shared.settings.set_choices(
        "cfg.mode",
        std::vec![
            Cow::Borrowed("cfg.mode.a"),
            Cow::Borrowed(long),
            Cow::Borrowed("cfg.mode.b"),
        ]
    ));
    let (rows, end) = page(&kernel, schema::OP_CHOICES, "cfg.mode");
    assert_eq!(rows.len(), 2);
    assert_eq!(end.total, 2);
    let detail = detail_as(&kernel, "reader", schema::OP_SETTING, "cfg.mode").unwrap();
    assert!(detail[1].ends_with("choices=2"));
}

#[test]
fn presets_are_listed_by_kind_and_one_has_a_description() {
    let mut kernel = kernel();
    // The built-in power modes come first, then those of `cfg` as added.
    let (rows, end) = page(&kernel, schema::OP_PRESETS, "");
    assert_eq!(end.total, rows.len() as u32);
    assert!(rows[0].starts_with("item core."));
    assert_eq!(
        rows.iter()
            .filter(|r| r.starts_with("item cfg."))
            .collect::<Vec<_>>(),
        [
            "item cfg.night cfg 0 cfg.preset.night",
            "item cfg.day cfg 0 Day",
            "item cfg.dark cfg 0 Dark"
        ]
    );
    let (rows, _) = page(&kernel, schema::OP_PRESETS, "cfg.power");
    assert_eq!(rows.len(), 2);
    let (rows, end) = page(&kernel, schema::OP_PRESETS, "nobody.kind");
    assert!(rows.is_empty() && end.total == 0);
    let rows = detail_as(&kernel, "reader", schema::OP_PRESET, "cfg.night").unwrap();
    assert_eq!(
        rows,
        [
            "item cfg.night cfg 0 cfg.preset.night",
            "text cfg.preset.night.help"
        ]
    );
    // No description: the row alone.
    assert_eq!(
        detail_as(&kernel, "reader", schema::OP_PRESET, "cfg.day")
            .unwrap()
            .len(),
        1
    );
    for unknown in ["cfg.nope", "", "cfg"] {
        assert_eq!(
            detail_as(&kernel, "reader", schema::OP_PRESET, unknown),
            Err(ERR_NOT_FOUND)
        );
    }
    kernel.shared.translator.add(
        paperpaw_i18n::Catalog::parse(Cow::Borrowed(
            "[meta]\nlanguage = de\nname = Deutsch\nplural = one-other\n\n[messages]\n\
             cfg.preset.night = Nacht\ncfg.preset.night.help = Zu Bett.\n",
        ))
        .unwrap(),
    );
    kernel.shared.translator.set_language("de");
    let rows = detail_as(&kernel, "reader", schema::OP_PRESET, "cfg.night").unwrap();
    assert_eq!(rows, ["item cfg.night cfg 0 Nacht", "text Zu Bett."]);
    let (rows, _) = page(&kernel, schema::OP_PRESETS, "cfg.power");
    assert_eq!(rows[0], "item cfg.night cfg 0 Nacht");
    // The kind of the built-in ones.
    let (rows, _) = page(&kernel, schema::OP_PRESETS, "core.power-mode");
    assert!(rows.len() >= 3 && rows.iter().all(|r| r.starts_with("item core.")));
}

#[test]
fn a_preset_sets_the_known_values_it_names_once_each() {
    let kernel = kernel();
    let (rows, end) = page(&kernel, schema::OP_VALUES, "cfg.night");
    // `cfg.size` is named three times, the last line fits no number: the
    // second line is its value. The comment and the unknown setting are not
    // rows.
    assert_eq!(
        rows,
        [
            "value cfg.size Int(22) 0",
            "value cfg.mode Text(\"cfg.mode.b\") 0"
        ]
    );
    assert_eq!((end.total, end.flags), (2, 1));
    // The scope names a preset only without the placement flag.
    let (rows, end) = page_as(&kernel, "reader", schema::OP_VALUES, "cfg.night", 1);
    assert!(rows.is_empty() && end.total == 0);
    for scope in ["", "cfg.nope", "cfg.size", "cfg.night@2"] {
        let (rows, end) = page(&kernel, schema::OP_VALUES, scope);
        assert!(rows.is_empty() && end.total == 0, "{scope}");
    }
}

#[test]
fn preset_values_follow_the_same_authority_as_settings() {
    let mut kernel = kernel();
    kernel
        .shared
        .settings
        .define(SettingSpec::switch(
            "core.grant.style.cfg",
            "grant",
            false,
            OTHER,
        ))
        .unwrap();
    // `cfg.dark` sets a grant, a secret and a plain switch.
    let expect = |plugin: &str, shown: bool| {
        let (rows, _) = page_as(&kernel, plugin, schema::OP_VALUES, "cfg.dark", 0);
        assert_eq!(rows.len(), 3, "{plugin}");
        let grant = if shown {
            "value core.grant.style.cfg Bool(true) 0"
        } else {
            "value core.grant.style.cfg - 2"
        };
        // A secret is withheld from everyone.
        assert_eq!(rows[0], grant, "{plugin}");
        assert_eq!(rows[1], "value cfg.key - 2", "{plugin}");
        assert_eq!(rows[2], "value cfg.flag Bool(false) 0", "{plugin}");
    };
    expect("reader", false);
    expect("cfg", false);
    expect("ed", true);
}

#[test]
fn a_placement_lists_only_the_values_it_sets_itself() {
    let mut kernel = kernel();
    let settings = &mut kernel.shared.settings;
    settings
        .set("cfg.clock.face", SettingValue::Text("kept".into()))
        .unwrap();
    settings
        .set("cfg.clock@2.face", SettingValue::Text("two".into()))
        .unwrap();
    settings
        .set("cfg.clock@3.size", SettingValue::Int(7))
        .unwrap();
    let (rows, end) = page_as(&kernel, "reader", schema::OP_VALUES, "cfg.clock@2", 1);
    assert_eq!(rows, ["value cfg.clock@2.face Text(\"two\") 0"]);
    assert_eq!((end.total, end.flags), (1, 1));
    let (rows, _) = page_as(&kernel, "reader", schema::OP_VALUES, "cfg.clock@3", 1);
    assert_eq!(rows, ["value cfg.clock@3.size Int(7) 0"]);
    // None of its own, a placement of a widget that is not there, and one
    // asked for without the placement flag: empty pages.
    for (scope, flags) in [
        ("cfg.clock@4", 1),
        ("nobody.x@1", 1),
        ("cfg.clock@2", 0),
        ("cfg.clock@", 1),
        ("@2", 1),
    ] {
        let (rows, end) = page_as(&kernel, "reader", schema::OP_VALUES, scope, flags);
        assert!(rows.is_empty() && end.total == 0, "{scope}/{flags}");
    }
}

#[test]
fn a_value_too_long_for_a_values_row_is_not_listed() {
    let mut kernel = kernel();
    let long = "y".repeat(600);
    kernel
        .shared
        .settings
        .set("cfg.clock@2.face", SettingValue::Text(long.into()))
        .unwrap();
    kernel
        .shared
        .settings
        .set("cfg.clock@2.size", SettingValue::Int(5))
        .unwrap();
    let (rows, end) = page_as(&kernel, "reader", schema::OP_VALUES, "cfg.clock@2", 1);
    assert_eq!(rows, ["value cfg.clock@2.size Int(5) 0"]);
    assert_eq!(end.total, 1);
}

/// A `reg_query` flag bit an op does not know is `ERR_UNSUPPORTED`, on every
/// list op; the bits it reads are fine.
#[test]
fn unknown_query_flag_bits_are_unsupported_on_every_list_op() {
    let kernel = kernel();
    let with = |scope: &str, flags: u32| {
        counted(Record::ServiceRegQuery(wire::ServiceRegQuery {
            scope,
            cursor: 0,
            generation: 0,
            flags,
        }))
    };
    for id in [
        schema::OP_SETTINGS,
        schema::OP_CHOICES,
        schema::OP_GROUPS,
        schema::OP_PLUGINS,
    ] {
        assert_eq!(
            call_as(&kernel, "reader", id, &with("", 2), 4096),
            Err(ERR_UNSUPPORTED),
            "{id}"
        );
        assert_eq!(
            call_as(&kernel, "reader", id, &with("", 1), 4096),
            Err(ERR_UNSUPPORTED),
            "{id}"
        );
    }
    // Op 18 reads bit 0 only.
    assert!(call_as(&kernel, "reader", schema::OP_VALUES, &with("", 1), 4096).is_ok());
    assert_eq!(
        call_as(&kernel, "reader", schema::OP_VALUES, &with("", 2), 4096),
        Err(ERR_UNSUPPORTED)
    );
}
