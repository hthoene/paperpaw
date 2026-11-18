//! Settings saved, plugins switched on and off, and data plugins loaded and installed.

use super::*;

#[test]
fn changed_settings_are_saved_and_announced() {
    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with("paperpaw.ini", "# my settings\n[core]\nui-size = 12\n");
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .build()
        .unwrap();
    assert_eq!(kernel.settings().int("core.ui-size"), 12);
    kernel.start(Instant(0));
    log.borrow_mut().clear();

    // Long tap in the centre runs core.toggle-night-mode.
    let down = InputEvent::Pointer(PointerEvent::new(
        0,
        PointerPhase::Down,
        Point::new(45, 45),
        PointerTool::Finger,
        1000,
    ));
    kernel.handle(Instant(100), down);
    kernel.tick(Instant(600));
    assert!(kernel.settings().bool("core.night-mode"));
    assert!(
        log.borrow()
            .contains(&String::from("event core.setting-changed core.night-mode"))
    );

    let mut buf = [0u8; 256];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    assert_eq!(
        core::str::from_utf8(&buf[..n]).unwrap(),
        "# my settings\n[core]\nui-size = 12\nnight-mode = on\n"
    );
}

#[test]
fn the_sole_plugin_asking_questions_cannot_be_switched_off() {
    // Nothing else asks yes or no or offers a choice: switched off, every
    // question would end unasked.
    let mut kernel = Kernel::builder(DEVICE)
        .items(
            Manifest::new(Id::new_static("ask"), "Ask", Version::new(1, 0, 0)),
            |r: &mut Staging| {
                r.add_fake(crate::testing::FakeSpec::new(
                    Id::new_static("ask.sheet"),
                    "sheet",
                    Box::new(|_, _| Box::new(HalfOverlay)),
                ))?;
                r.bind_slot(
                    slots::CHOICE,
                    Id::new_static("ask.sheet"),
                    Priority::DEFAULT,
                )?;
                r.bind_slot(
                    slots::CONFIRM,
                    Id::new_static("ask.sheet"),
                    Priority::DEFAULT,
                )
            },
        )
        .build()
        .unwrap();
    let ask = kernel.plugins().get("ask").unwrap();
    assert_eq!(
        ask.required_for.iter().collect::<Vec<_>>(),
        [&slots::CHOICE, &slots::CONFIRM]
    );
    assert!(!ask.can_switch_off());
    kernel.set_plugin_enabled(&Id::new_static("ask"), false);
    assert!(kernel.plugins().get("ask").unwrap().enabled);
}

#[test]
fn plugins_switch_on_and_off_for_the_next_start() {
    use paperpaw_sdk::PluginState::{Off, Running};

    let log = log();
    let storage = paperpaw_sdk::MemoryStorage::new().with(
        "paperpaw.ini",
        "[plugins]
b = off
",
    );
    let mut kernel = Kernel::builder(DEVICE)
        .storage(storage)
        .items(
            overlay_plugin_manifest(),
            overlay_plugin(OverlayPlugin { log: log.clone() }),
        )
        .test_plugin(TestPlugin::new("a", &log))
        .test_plugin(TestPlugin::new("b", &log))
        .build()
        .unwrap();
    let states: Vec<(&str, paperpaw_sdk::PluginState)> = kernel
        .plugins()
        .iter()
        .map(|p| (p.manifest.id.as_str(), p.state))
        .collect();
    assert_eq!(
        states,
        [
            ("core", Running),
            ("ov", Running),
            ("a", Running),
            ("b", Off)
        ]
    );
    assert!(kernel.settings().get("b.level").is_none());
    // "ov" alone provides the menu; the home screen has two providers.
    let ov = kernel.plugins().get("ov").unwrap();
    assert_eq!(ov.required_for.iter().collect::<Vec<_>>(), [&slots::MENU]);
    assert!(!kernel.plugins().restart_needed());

    kernel.apply(Ask::SetPluginEnabled(Id::new_static("a"), false));
    kernel.apply(Ask::SetPluginEnabled(Id::new_static("b"), true));
    // The kernel and plugins that alone provide a slot stay on.
    kernel.apply(Ask::SetPluginEnabled(Id::new_static("core"), false));
    kernel.apply(Ask::SetPluginEnabled(Id::new_static("ov"), false));
    assert_eq!(kernel.take_warnings().len(), 2);
    assert!(kernel.plugins().get("ov").unwrap().enabled);
    assert!(kernel.plugins().get("a").unwrap().restart_needed());
    assert!(kernel.plugins().restart_needed());

    let mut buf = [0u8; 64];
    let n = kernel
        .shared
        .storage
        .read_at("paperpaw.ini", 0, &mut buf)
        .unwrap();
    assert_eq!(
        core::str::from_utf8(&buf[..n]).unwrap(),
        "[plugins]
b = on
a = off
"
    );

    kernel.apply(Ask::Restart);
    assert!(kernel.finish().restart());
}

#[test]
fn data_plugins_load_from_storage() {
    use paperpaw_sdk::PluginState::{Off, Running};

    let files = |config: &str| {
        paperpaw_sdk::MemoryStorage::new()
            .with(
                "plugins/large/plugin.ini",
                "[plugin]
name = Large print
description = Bigger text.
",
            )
            .with(
                "plugins/large/settings.ini",
                "[core]
ui-size = 14
night-mode = on
",
            )
            .with(
                "plugins/large/i18n/de.ini",
                "[meta]
language = de
name = Deutsch
plural = one-other

[messages]
large.plugin.name = Großdruck
",
            )
            .with("paperpaw.ini", config)
    };

    // Its defaults apply; the user's own choice wins.
    let kernel = Kernel::builder(DEVICE)
        .storage(files(
            "[core]
night-mode = off
",
        ))
        .build()
        .unwrap();
    let large = kernel.plugins().get("large").unwrap();
    assert_eq!(large.state, Running);
    assert_eq!(large.manifest.name, "Large print");
    assert_eq!(kernel.settings().int("core.ui-size"), 14);
    assert!(!kernel.settings().bool("core.night-mode"));

    // Switched off: listed with its translated name, no defaults.
    let mut kernel = Kernel::builder(DEVICE)
        .storage(files(
            "[plugins]
large = off
",
        ))
        .build()
        .unwrap();
    kernel.shared.translator.set_language("de");
    let large = kernel.plugins().get("large").unwrap();
    assert_eq!(large.state, Off);
    assert_eq!(large.name(&kernel.shared.translator), "Großdruck");
    assert_ne!(kernel.settings().int("core.ui-size"), 14);
}

#[test]
fn data_plugins_load_and_go_while_running() {
    use paperpaw_sdk::PluginState::Running;
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};

    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    builder.add_strike(10.0, metrics, alloc::vec![('a', Glyph::empty(5.0))]);
    let pack = builder.build();

    let mut kernel = Kernel::builder(DEVICE)
        .storage(paperpaw_sdk::MemoryStorage::new())
        .build()
        .unwrap();
    kernel.take_warnings();
    // Downloaded while running: a family of one face.
    let storage = kernel.shared.storage.clone();
    storage
        .write(
            "plugins/font-round/plugin.ini",
            b"[plugin]
name = Round

[family]
name = Round
regular = round.ppfp
",
        )
        .unwrap();
    storage
        .write("plugins/font-round/fonts/round.ppfp", &pack)
        .unwrap();
    kernel.apply(Ask::LoadPlugin("plugins/font-round".into()));
    assert!(kernel.take_warnings().is_empty());
    assert_eq!(kernel.plugins().get("font-round").unwrap().state, Running);
    assert!(kernel.shared.fonts.families().contains(&"Round"));
    assert!(kernel.output.commands.iter().any(
        |c| matches!(c, Ask::Emit(e, _) if *e == paperpaw_sdk::builtin::events::PLUGINS_CHANGED)
    ));

    // Chosen for reading at once.
    kernel.apply(Ask::SetSetting(
        settings::READING_FONT.id().clone(),
        SettingValue::Text("Round".into()),
    ));
    assert!(kernel.take_warnings().is_empty());
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert_eq!(reading.id().as_str(), "font-round.regular");

    // Removed: its folder, its fonts, and the choice gives way.
    kernel.apply(Ask::RemovePlugin(Id::new_static("font-round")));
    assert!(kernel.take_warnings().is_empty());
    assert!(!storage.exists("plugins/font-round/plugin.ini"));
    assert!(kernel.plugins().get("font-round").is_none());
    assert!(!kernel.shared.fonts.families().contains(&"Round"));
    assert_eq!(
        kernel.settings().text(settings::READING_FONT.as_str()),
        settings::DEFAULT_FONT
    );

    // Plugins with code stay.
    kernel.apply(Ask::RemovePlugin(Id::new_static("core")));
    assert_eq!(kernel.take_warnings().len(), 1);
    assert!(kernel.plugins().get("core").is_some());
}

/// A font plugin `font-round` in `version`, its family Round.
fn round_plugin(version: &str, extra: &str) -> String {
    alloc::format!(
        "[plugin]
id = font-round
name = Round
version = {version}
{extra}
[family]
name = Round
regular = round.ppfp
"
    )
}

fn round_pack() -> Vec<u8> {
    use paperpaw_text::{Glyph, LineMetrics, PackBuilder};
    let mut builder = PackBuilder::new();
    let metrics = LineMetrics::new(8.0, 2.0, 12.0);
    builder.add_strike(10.0, metrics, alloc::vec![('a', Glyph::empty(5.0))]);
    builder.build()
}

/// Puts version `version` of the font plugin where an install takes it
/// from.
fn stage_round(storage: &dyn paperpaw_platform::Storage, pack: &[u8], version: &str) {
    storage
        .write(
            "plugins/.font-round.part/plugin.ini",
            round_plugin(version, "").as_bytes(),
        )
        .unwrap();
    storage
        .write("plugins/.font-round.part/fonts/round.ppfp", pack)
        .unwrap();
}

#[test]
fn data_plugins_are_installed_in_place_of_every_version() {
    use paperpaw_sdk::PluginState::Running;

    let pack = round_pack();
    let storage = paperpaw_sdk::MemoryStorage::new()
        // Loaded at start, in a folder of another name.
        .with("plugins/round-old/plugin.ini", round_plugin("1.0.0", ""))
        .with("plugins/round-old/fonts/round.ppfp", pack.clone())
        // Refused at start: made for later formats.
        .with(
            "plugins/round-later/plugin.ini",
            round_plugin("3.0.0", "formats = 99"),
        )
        .with("plugins/other/plugin.ini", "[plugin]\nname = Other\n");
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    kernel.take_warnings();
    kernel.apply(Ask::SetSetting(
        settings::READING_FONT.id().clone(),
        SettingValue::Text("Round".into()),
    ));
    let storage = kernel.shared.storage.clone();
    let stage = |version: &str| stage_round(storage.as_ref(), &pack, version);

    // A newer version: in its own folder, the others gone, the font one
    // reads with still chosen.
    stage("1.1.0");
    kernel.apply(Ask::InstallPlugin(
        Id::new_static("font-round"),
        "plugins/.font-round.part".into(),
    ));
    assert!(kernel.take_warnings().is_empty());
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!((round.state, round.manifest.version.minor), (Running, 1));
    assert!(storage.exists("plugins/font-round/fonts/round.ppfp"));
    for gone in [
        "plugins/round-old/plugin.ini",
        "plugins/round-later/plugin.ini",
        "plugins/.font-round.part/plugin.ini",
        "plugins/.font-round.old/plugin.ini",
    ] {
        assert!(!storage.exists(gone), "{gone}");
    }
    assert!(storage.exists("plugins/other/plugin.ini"));
    assert_eq!(
        kernel.settings().text(settings::READING_FONT.as_str()),
        "Round"
    );
    let reading = kernel
        .shared
        .fonts
        .role(paperpaw_sdk::builtin::fonts::READING.as_str())
        .unwrap();
    assert_eq!(reading.id().as_str(), "font-round.regular");
}

#[test]
fn data_plugins_switched_off_are_updated_in_place() {
    use paperpaw_sdk::PluginState::Off;

    let pack = round_pack();
    // Updated while switched off: it stays off, in its folder.
    let storage = paperpaw_sdk::MemoryStorage::new()
        .with(
            "paperpaw.ini",
            "[plugins]
font-round = off
",
        )
        .with("plugins/font-round/plugin.ini", round_plugin("1.1.0", ""))
        .with("plugins/font-round/fonts/round.ppfp", pack.clone())
        .with(
            "plugins/other/plugin.ini",
            "[plugin]
name = Other
",
        );
    let mut kernel = Kernel::builder(DEVICE).storage(storage).build().unwrap();
    assert_eq!(kernel.plugins().get("font-round").unwrap().state, Off);
    let storage = kernel.shared.storage.clone();
    let stage = |version: &str| stage_round(storage.as_ref(), &pack, version);
    stage("1.2.0");
    kernel.apply(Ask::InstallPlugin(
        Id::new_static("font-round"),
        "plugins/.font-round.part".into(),
    ));
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!((round.state, round.manifest.version.minor), (Off, 2));
    assert!(storage.exists("plugins/font-round/plugin.ini"));
    assert!(!storage.exists("plugins/.font-round.part/plugin.ini"));

    // A folder holding another plugin than it says: refused, the version
    // before stays.
    kernel.take_warnings();
    stage("2.0.0");
    kernel.apply(Ask::InstallPlugin(
        Id::new_static("other"),
        "plugins/.font-round.part".into(),
    ));
    assert_eq!(kernel.take_warnings().len(), 1);
    assert!(!storage.exists("plugins/.font-round.part/plugin.ini"));
    let round = kernel.plugins().get("font-round").unwrap();
    assert_eq!(round.manifest.version.minor, 2);
    assert!(storage.exists("plugins/other/plugin.ini"));
}
