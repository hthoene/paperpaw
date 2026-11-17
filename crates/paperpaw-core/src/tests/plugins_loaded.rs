//! Plugins as they are loaded and installed: ids that may not be taken,
//! versions replaced, what is left of them on the storage.

use super::*;
use paperpaw_sdk::MemoryStorage;

/// A plugin, built in or a data plugin, may not take the name of a
/// section of `paperpaw.ini` that holds no plugin's settings: they would
/// be read as menus, slots or fonts there.
#[test]
fn plugins_may_not_take_the_names_of_config_sections() {
    let log = log();
    let storage = MemoryStorage::new().with("plugins/fonts/plugin.ini", "[plugin]\nname = Fonts\n");
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("menus", &log))
        .storage(storage)
        .build()
        .unwrap();
    for id in ["menus", "fonts"] {
        let info = kernel.plugins().get(id).unwrap();
        assert_eq!(info.state, paperpaw_sdk::PluginState::Failed, "{id}");
    }
    assert!(kernel.settings().get("menus.level").is_none());
    let warnings = kernel.take_warnings();
    assert_eq!(
        warnings
            .iter()
            .filter(|w| w.message.contains("names a section"))
            .count(),
        2,
        "{warnings:?}"
    );
}

/// An install cut short by the power (the old version set aside, the new
/// one not yet in its place) loses nothing: the old version is put back
/// at the next start. Where the new one got there, what was set aside
/// goes.
#[test]
fn a_plugin_set_aside_by_an_install_cut_short_is_put_back() {
    let manifest = "[plugin]\nname = Large print\n";
    let storage = MemoryStorage::new()
        .with("plugins/.large.old/plugin.ini", manifest)
        .with(
            "plugins/.small.old/plugin.ini",
            "[plugin]\nversion = 1.0.0\n",
        )
        .with("plugins/small/plugin.ini", "[plugin]\nversion = 2.0.0\n");
    let kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    let storage = kernel.storage();
    assert!(kernel.plugins().get("large").is_some());
    assert!(storage.exists("plugins/large/plugin.ini"));
    assert!(!storage.exists("plugins/.large.old/plugin.ini"));
    let small = kernel.plugins().get("small").unwrap();
    assert_eq!(small.manifest.version, Version::new(2, 0, 0));
    assert!(!storage.exists("plugins/.small.old/plugin.ini"));
}

/// A data plugin switched off keeps of its translations only its name
/// and description, to be listed: the rest (a language pack's whole
/// catalogs) would take memory for nothing.
#[test]
fn a_data_plugin_switched_off_keeps_only_what_lists_it() {
    let catalog = "[meta]\nlanguage = en\nname = English\n\n[messages]\nbig.plugin.name = Big print\nbig.other = Taken\n";
    let storage = MemoryStorage::new()
        .with("plugins/big/plugin.ini", "[plugin]\nname = Big\n")
        .with("plugins/big/i18n/en.ini", catalog);
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[plugins]\nbig = off\n")
        .build()
        .unwrap();
    let info = kernel.plugins().get("big").unwrap();
    assert_eq!(info.state, paperpaw_sdk::PluginState::Off);
    assert_eq!(
        kernel.translator().get("big.plugin.name"),
        Some("Big print")
    );
    assert_eq!(kernel.translator().get("big.other"), None);
}

/// A kernel with the data plugin `big` installed, version 1.
fn installed_kernel(storage: MemoryStorage, log: &Log) -> Kernel {
    let storage = storage
        .with("plugins/big/plugin.ini", "[plugin]\nversion = 1.0.0\n")
        .with(
            "plugins/big/i18n/en.ini",
            "[meta]\nlanguage = en\nname = English\n\n[messages]\nbig.plugin.name = Big\n",
        );
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", log))
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    kernel.take_warnings();
    kernel
}

fn install(kernel: &mut Kernel, id: &'static str, folder: &str) {
    kernel.apply(Command::InstallPlugin(
        Id::new_static(id),
        String::from(folder),
    ));
    kernel.finish();
}

/// An update that would not register (a translation with a syntax error)
/// is refused before anything changes: the version before stays.
#[test]
fn an_update_that_would_not_register_leaves_the_version_before() {
    let log = log();
    let storage = MemoryStorage::new()
        .with(
            "plugins/.big.part/plugin.ini",
            "[plugin]\nid = big\nversion = 2.0.0\n",
        )
        .with("plugins/.big.part/i18n/de.ini", "[meta\nlanguage = de\n");
    let mut kernel = installed_kernel(storage, &log);
    install(&mut kernel, "big", "plugins/.big.part");
    let big = kernel.plugins().get("big").unwrap();
    assert_eq!(big.manifest.version, Version::new(1, 0, 0));
    assert_eq!(big.state, paperpaw_sdk::PluginState::Running);
    assert!(kernel.storage().exists("plugins/big/plugin.ini"));
    assert!(!kernel.storage().exists("plugins/.big.part/plugin.ini"));
    assert!(!kernel.take_warnings().is_empty());
}

/// A data plugin with the id of a plugin built in is refused: it would
/// unload that plugin's fonts, translations and listing.
#[test]
fn a_data_plugin_may_not_take_the_place_of_one_built_in() {
    let log = log();
    let storage = MemoryStorage::new().with(
        "plugins/.a.part/plugin.ini",
        "[plugin]\nid = a\nname = Fake\n",
    );
    let mut kernel = installed_kernel(storage, &log);
    install(&mut kernel, "a", "plugins/.a.part");
    let a = kernel.plugins().get("a").unwrap();
    assert_eq!(a.state, paperpaw_sdk::PluginState::Running);
    assert_eq!(a.manifest.name, "Test");
    assert!(!kernel.storage().exists("plugins/a/plugin.ini"));
    assert!(!kernel.storage().exists("plugins/.a.part/plugin.ini"));
}

/// Installing a plugin from the folder it is installed in changes
/// nothing: it is not removed.
#[test]
fn installing_a_plugin_from_its_own_folder_keeps_it() {
    let log = log();
    let mut kernel = installed_kernel(MemoryStorage::new(), &log);
    install(&mut kernel, "big", "plugins/big");
    assert!(kernel.storage().exists("plugins/big/plugin.ini"));
    assert!(kernel.plugins().get("big").is_some());
    assert!(!kernel.take_warnings().is_empty());
}

/// A data plugin updated or removed while running takes its stylesheet
/// and key bindings with it: only the new version's apply.
#[test]
fn a_plugin_updated_or_removed_takes_its_styles_and_bindings_along() {
    let log = log();
    let storage = MemoryStorage::new()
        .with("plugins/big/styles.css", "p { line-height: 2 }")
        .with(
            "plugins/big/keymap.ini",
            "[keymap.global]\nbutton.menu = core.toggle-night-mode\n",
        )
        .with(
            "plugins/.big.part/plugin.ini",
            "[plugin]\nid = big\nversion = 2.0.0\n",
        )
        .with("plugins/.big.part/styles.css", "p { line-height: 3 }");
    let mut kernel = installed_kernel(storage, &log);
    let menu = crate::keymap::InputKey::Button(Cow::Borrowed("menu"));
    assert_eq!(kernel.shared.stylesheets, ["p { line-height: 2 }"]);
    assert_eq!(kernel.resolve(&menu), Some(&actions::TOGGLE_NIGHT_MODE));
    install(&mut kernel, "big", "plugins/.big.part");
    assert_eq!(kernel.shared.stylesheets, ["p { line-height: 3 }"]);
    assert_ne!(kernel.resolve(&menu), Some(&actions::TOGGLE_NIGHT_MODE));
    kernel.apply(Command::RemovePlugin(Id::new_static("big")));
    kernel.finish();
    assert!(kernel.shared.stylesheets.is_empty());
}

/// A theme's defaults win over the device's: a data plugin for larger
/// print enlarges the interface also where the device's own default is
/// small (the X4's). What the user chose still wins over both.
#[test]
fn a_themes_defaults_win_over_the_devices() {
    let storage = MemoryStorage::new()
        .with("plugins/large/plugin.ini", "[plugin]\nname = Large print\n")
        .with(
            "plugins/large/settings.ini",
            "[core]\nui-size = 14\nfont-size = 16\n",
        );
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .setting_default(&settings::UI_SIZE, 9)
        .setting_default(&settings::FONT_SIZE, 9)
        .user_config("[core]\nfont-size = 12\n")
        .build()
        .unwrap();
    assert_eq!(kernel.settings().int("core.ui-size"), 14);
    assert_eq!(kernel.settings().int("core.font-size"), 12, "the user's");
    // Where each default comes from, for a settings page to tell: the
    // theme's for both (the user's value over one aside), none for a
    // setting it leaves alone.
    let from = |id| kernel.settings().default_from(id).map(Id::as_str);
    assert_eq!(from("core.ui-size"), Some("large"));
    assert_eq!(from("core.font-size"), Some("large"));
    assert_eq!(from("core.line-spacing"), None);
}

/// A plugin's defaults for its own settings are the settings' own: none
/// is named as coming from it; a plugin switched off gives none.
#[test]
fn defaults_name_the_plugin_they_come_from_only_for_others_settings() {
    let storage = MemoryStorage::new()
        .with("plugins/large/plugin.ini", "[plugin]\nname = Large print\n")
        .with("plugins/large/settings.ini", "[core]\nui-size = 14\n")
        .with("plugins/calm/plugin.ini", "[plugin]\nname = Calm\n")
        .with("plugins/calm/settings.ini", "[core]\nfont-size = 15\n");
    let kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .user_config("[plugins]\ncalm = off\n")
        .build()
        .unwrap();
    let mut settings = paperpaw_sdk::Settings::new();
    settings
        .define(paperpaw_sdk::SettingSpec::number(
            "own.size",
            "x",
            0,
            9,
            1,
            paperpaw_sdk::builtin::groups::OTHER,
        ))
        .unwrap();
    settings
        .set_default_by("own.size", "4", Id::new_static("own"))
        .unwrap();
    assert_eq!(settings.int("own.size"), 4);
    assert_eq!(settings.default_from("own.size"), None);
    assert!(
        settings
            .set_default_by("own.size", "x", Id::new_static("b"))
            .is_err()
    );
    assert_eq!(settings.default_from("own.size"), None, "not taken");
    assert_eq!(
        kernel
            .settings()
            .default_from("core.ui-size")
            .map(Id::as_str),
        Some("large")
    );
    assert_eq!(
        kernel.settings().default_from("core.font-size"),
        None,
        "switched off"
    );
}

/// A data plugin marks words by patterns: its `marks.ini` makes roles,
/// their looks settings, and a decorator.
#[test]
fn a_data_plugin_marks_words_by_patterns() {
    let storage = MemoryStorage::new()
        .with(
            "plugins/tale/plugin.ini",
            "[plugin]\nname = Tale\nformats = 2\n",
        )
        .with(
            "plugins/tale/marks.ini",
            "[names]\ntitle = Names\npattern = \\bAlice\\b\nbackground = yellow\n",
        );
    let kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert_eq!(
        kernel.settings().text("tale.names.background"),
        "core.tone.yellow"
    );
    let decorators = &kernel.shared.decorators;
    assert!(decorators.get(&Id::new_static("tale.marks")).is_some());
}

/// A data plugin of formats 3: actions made of steps, a menu of its own
/// with entries placed after others and taken out, icons, lines for a
/// point, a step of the first-run setup.
#[test]
fn data_plugins_bring_actions_menus_icons_points_and_steps() {
    let log = log();
    let storage = MemoryStorage::new()
        .with(
            "plugins/cosy/plugin.ini",
            "[plugin]\nname = Cosy\nformats = 3\n",
        )
        .with(
            "plugins/cosy/actions.ini",
            "[night]\ntitle = Night reading\nicon = moon\nrun = a.level=4\nrun = notify:cosy.good-night\nrun = core.confirm\nrun = a.level=nonsense\n",
        )
        .with(
            "plugins/cosy/menus.ini",
            "[cosy.tools]\ntitle = Tools\nentries = cosy.night\nentries = a.special, core.confirm\n[menu cosy.tools]\nadd = core.home after cosy.night\nhide = a.special\n",
        )
        .with("plugins/cosy/icons.ini", "a.special = star\n")
        .with(
            "plugins/cosy/points/core.about.txt",
            "Made with care\n# not a line\n\nSecond line\n",
        )
        .with(
            "plugins/cosy/setup.ini",
            "[size]\nsetting = a.level\norder = 35\ntitle = How large\n",
        );
    let mut kernel = Kernel::builder(DEVICE)
        .test_plugin(TestPlugin::new("a", &log))
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let warnings = kernel.take_warnings();
    assert!(warnings.is_empty(), "{warnings:?}");
    let menus = &kernel.shared.menus;
    let tools: Vec<&str> = menus
        .default_ids("cosy.tools")
        .into_iter()
        .map(Id::as_str)
        .collect();
    assert_eq!(tools, ["cosy.night", "core.home", "core.confirm"]);
    assert_eq!(menus.icon("cosy.night"), Some("moon"));
    assert_eq!(menus.icon("a.special"), Some("star"));
    let about: Vec<&str> = kernel
        .shared
        .services
        .contributions("core.about")
        .map(|(_, line)| line)
        .collect();
    assert_eq!(about, ["Made with care", "Second line"]);
    let env = kernel.shared.place(Instant(0));
    let steps: Vec<&str> = kernel
        .shared
        .setup
        .applying(&env)
        .iter()
        .map(|step| step.id.as_str())
        .collect();
    assert!(steps.contains(&"cosy.size"), "{steps:?}");
    log.borrow_mut().clear();
    kernel.run_action(Instant(10), Id::new_static("cosy.night"));
    assert_eq!(
        kernel.settings().get("a.level"),
        Some(&SettingValue::Int(4))
    );
    assert_eq!(*log.borrow(), ["a:cosy.night", "a:core.confirm"]);
}

/// What the store hears of plugins asked for by id (where it `installs`
/// plugins), and the answers anyone hears.
pub(super) struct Store {
    pub(super) log: Log,
    pub(super) installs: bool,
}

pub(super) fn store(kept: Store) -> impl Fn(&mut Items) -> Result<(), RegisterError> {
    move |r| {
        use paperpaw_sdk::builtin::events;
        crate::tests::hear_into(&kept.log);
        r.on(events::PLUGINS_ANSWERED, |_, p| {
            let Some(answer) = crate::hear::data::<events::PluginsAnswered>(p) else {
                return;
            };
            let ids = |list: Vec<Id>| list.iter().map(Id::as_str).collect::<Vec<_>>().join(",");
            crate::tests::heard(alloc::format!(
                "{} got [{}] not [{}] declined {} error {:?}",
                answer.by,
                ids(answer.installed_ids().collect()),
                ids(answer.missing_ids().collect()),
                answer.declined,
                answer.error
            ));
        })?;
        if !kept.installs {
            return Ok(());
        }
        r.on(events::PLUGINS_REQUESTED, |_, p| {
            if let Some(asked) = crate::hear::data::<events::PluginsRequested>(p) {
                let ids: Vec<String> = asked.ids().map(|id| id.to_string()).collect();
                crate::tests::heard(alloc::format!("{} asks {}", asked.by, ids.join(",")));
            }
        })
    }
}

/// What [`store`] registers under.
pub(super) fn store_manifest() -> Manifest {
    Manifest::new(Id::new_static("shop"), "Shop", Version::new(1, 0, 0))
}

/// Where all asked for are on the device, or no plugin installs
/// plugins, the kernel answers the plugin that asked itself; else the
/// store does, and the kernel says nothing.
#[test]
fn the_kernel_answers_requests_no_store_can() {
    let log = log();
    let storage = MemoryStorage::new().with("plugins/dictionary/plugin.ini", "[plugin]\n");
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            store_manifest(),
            store(Store {
                log: log.clone(),
                installs: false,
            }),
        )
        .storage(storage)
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let id = Id::new_static;
    kernel.apply(Command::request_plugins(id("reader"), [id("dictionary")]));
    kernel.apply(Command::request_plugins(
        id("reader"),
        [id("script-arabic"), id("dictionary"), id("script-hebrew")],
    ));
    kernel.run_commands();
    let error = kernel
        .translator()
        .tr("core.plugins.no-installer")
        .to_string();
    assert!(!error.is_empty() && error != "core.plugins.no-installer");
    assert_eq!(
        *log.borrow(),
        [
            String::from("reader got [] not [] declined false error \"\""),
            alloc::format!(
                "reader got [] not [script-arabic,script-hebrew] declined false error {error:?}"
            ),
        ]
    );
}

/// A plugin asks for plugins by id: the store hears those not on the
/// device, each once, with who asked; nothing where all are there.
#[test]
fn plugins_asked_for_by_id_go_to_the_store_if_not_on_the_device() {
    let log = log();
    let storage = MemoryStorage::new()
        .with("plugins/dictionary/plugin.ini", "[plugin]\n")
        .with("plugins/aids/plugin.ini", "[plugin]\n");
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            store_manifest(),
            store(Store {
                log: log.clone(),
                installs: true,
            }),
        )
        .storage(storage)
        .user_config("[plugins]\naids = off\n")
        .build()
        .unwrap();
    kernel.start(Instant(0));
    let id = Id::new_static;
    kernel.apply(Command::request_plugins(
        id("template-learning"),
        [
            id("vocabulary"),
            id("dictionary"),
            id("aids"),
            id("core"),
            id("shop"),
            id("vocabulary"),
            id("quotes"),
        ],
    ));
    kernel.run_commands();
    // On the device (switched off too), the kernel, twice: left out.
    assert_eq!(*log.borrow(), ["template-learning asks vocabulary,quotes"]);
    log.borrow_mut().clear();
    kernel.apply(Command::request_plugins(id("x"), [id("dictionary")]));
    kernel.apply(Command::request_plugins(id("x"), []));
    kernel.run_commands();
    // All there: answered at once, nothing for the store.
    let none = "x got [] not [] declined false error \"\"";
    assert_eq!(*log.borrow(), [none, none]);
    log.borrow_mut().clear();
    // More than one request takes: the rest left out, with a warning.
    let many = (0..40).map(|i| Id::parse(&alloc::format!("p{i}")).unwrap());
    kernel.apply(Command::request_plugins(id("x"), many));
    kernel.run_commands();
    let heard = log.borrow()[0].clone();
    assert_eq!(
        heard.matches(',').count() + 1,
        paperpaw_sdk::MAX_REQUESTED_PLUGINS
    );
    assert!(
        kernel
            .take_warnings()
            .iter()
            .any(|w| w.message.contains("more than 32"))
    );
}
