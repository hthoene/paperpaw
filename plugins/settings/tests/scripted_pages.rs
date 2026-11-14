//! The settings pages of the native plugin over the scripted host: search,
//! choosing a value, typed text, themes, a group's actions, covered pages and
//! a screen shown again at another size. What the interface-6 suites `pages`,
//! `search`, `groups`, `covered` and `resize` pinned through a kernel is read
//! here from what the plugin asks of the registry and `core.ui` and from the
//! commands it submits. The scripted host has no screen stack, so the screen
//! commands that follow (open, back, redraw) are refused (BLOCKED H1) and the
//! focus can only start where a hand-over puts it (`focus`) or on the first
//! row; drawing is not run (BLOCKED H1/H4, `acceptance_pending.rs`).
#![cfg(feature = "linked")]
#![allow(
    clippy::unwrap_used,
    clippy::field_reassign_with_default,
    reason = "test assertions; a spec is filled field by field"
)]
mod scripted_world;
use scripted_world::{
    ADVANCED, CHOOSE, G, GROUP, LIST, M, S, SEARCH, Spec, THEMES, V, World, answered, changed,
    commands, finished, press, registry_changed, shown,
};

const OPEN: &str = "core.confirm";

/// The settings of the app that the searches look through.
fn app() -> Spec {
    let mut spec = Spec::default();
    spec.groups = vec![
        G::new("core.group.reading", "Reading", 10),
        G::new("core.group.look", "Appearance", 20),
        G::new("calibre.group", "Calibre", 0),
    ];
    spec.settings = vec![
        S::number(
            "core.margins",
            "core.group.reading",
            "Margin size",
            3,
            (0, 9),
        ),
        S::number(
            "core.font-size",
            "core.group.reading",
            "Font size",
            18,
            (8, 40),
        )
        .help("The size of the letters"),
        S::number(
            "core.font-weight",
            "core.group.reading",
            "Font weight",
            4,
            (1, 9),
        ),
        S::choice(
            "core.week-start",
            "core.group.reading",
            "Week starts on",
            "core.week.auto",
            &[
                ("core.week.auto", "As the language has it"),
                ("core.week.monday", "Monday"),
                ("core.week.sunday", "Sunday"),
            ],
        ),
        S::switch("core.night-mode", "core.group.look", "Night mode", false),
        S::choice(
            "core.page-frame",
            "core.group.look",
            "Page frame",
            "core.frame.none",
            &[("core.frame.none", "None")],
        ),
        S::choice(
            "core.clock-format",
            "core.group.look",
            "Clock",
            "24",
            &[("24", "24 hours"), ("12", "12 hours")],
        ),
        S::switch("calibre.sync", "calibre.group", "Sync", false),
        S::text("calibre.address", "calibre.group", "Address", "192.168.1.5").flags(ADVANCED),
    ];
    spec.menus.insert(
        "core.settings-pages".into(),
        vec![
            M::action("acct.page", "Password", 50),
            M::action("settings.secrets.open", "Passwords and keys", 95),
        ],
    );
    spec
}

/// What choosing the first hit of the search for `text` does: the hand-over
/// of the page it opens, or `None` when the search lists nothing.
fn first_hit(spec: &Spec, advanced: bool, text: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let world = World::new(spec, advanced);
    let plugin = world.load();
    world.open(&plugin, (SEARCH, "settings.search", 1), &[("text", text)]);
    let done = press(&plugin, SEARCH, OPEN);
    assert!(finished(done.status), "{}", done.status);
    (
        world.lines("handover"),
        commands(&done),
        world.lines("list 12"),
    )
}

#[test]
fn search_finds_settings_by_words_in_english() {
    // Every word counts: "size font" finds the font size, "size" alone finds
    // the margins first (the registry's order).
    let (hand, ..) = first_hit(&app(), false, "  SIZE   font ");
    assert_eq!(
        hand,
        ["handover id=core.group.reading focus=core.font-size"]
    );
    let (hand, ..) = first_hit(&app(), false, "size");
    assert_eq!(hand, ["handover id=core.group.reading focus=core.margins"]);
    // The description counts as well.
    let (hand, ..) = first_hit(&app(), false, "letters");
    assert_eq!(
        hand,
        ["handover id=core.group.reading focus=core.font-size"]
    );
}

#[test]
fn search_finds_settings_by_words_in_german_with_or_without_accents() {
    let mut spec = app();
    spec.settings[1] = S::number(
        "core.font-size",
        "core.group.reading",
        "Schriftgröße",
        18,
        (8, 40),
    );
    for query in ["schriftgröße", "SCHRIFTGROSSE", "Schriftgrosse"] {
        let (hand, ..) = first_hit(&spec, false, query);
        assert_eq!(
            hand,
            ["handover id=core.group.reading focus=core.font-size"],
            "{query}"
        );
    }
}

#[test]
fn a_hit_opens_its_page_with_the_setting_focused() {
    // A setting of a group that is registered: its page, the setting focused.
    let (hand, shown, _) = first_hit(&app(), false, "font size");
    assert_eq!(
        hand,
        ["handover id=core.group.reading focus=core.font-size"]
    );
    // The page opens as a screen (refused by the scripted host: H1).
    assert_eq!(shown, ["open settings.group"]);
    // A setting of a group that is not registered is on the page of "other".
    let mut spec = app();
    spec.settings
        .push(S::switch("lost.flag", "lost.nowhere", "Lost flag", false));
    let (hand, ..) = first_hit(&spec, false, "lost flag");
    assert_eq!(hand, ["handover id=core.group.other focus=lost.flag"]);
}

#[test]
fn search_finds_the_pages_of_other_plugins_and_opens_them() {
    let (hand, opened, _) = first_hit(&app(), false, "password");
    // A page, not a setting: its action runs, no page of settings opens.
    assert!(hand.is_empty(), "{hand:?}");
    assert_eq!(opened, ["run acct.page"]);
}

#[test]
fn a_search_without_hits_opens_nothing() {
    let (hand, opened, _) = first_hit(&app(), false, "zzqx flibbertigibbet");
    assert!(hand.is_empty() && opened.is_empty(), "{hand:?} {opened:?}");
    // Nothing typed: nothing found either.
    let (hand, opened, _) = first_hit(&app(), false, "   ");
    assert!(hand.is_empty() && opened.is_empty(), "{hand:?} {opened:?}");
}

#[test]
fn advanced_settings_are_found_only_once_shown() {
    // The address of Calibre is an advanced setting.
    let (before, ..) = first_hit(&app(), false, "address");
    assert!(before.is_empty(), "{before:?}");
    let (after, ..) = first_hit(&app(), true, "address");
    assert_eq!(after, ["handover id=calibre.group focus=calibre.address"]);
}

#[test]
fn a_choice_with_nothing_to_choose_is_not_listed() {
    // The page frame offers one column alone: nothing to choose.
    let (hand, ..) = first_hit(&app(), false, "page frame");
    assert!(hand.is_empty(), "{hand:?}");
    // A choice of two is (the clock's 24 or 12 hours).
    let (hand, ..) = first_hit(&app(), false, "clock");
    assert_eq!(
        hand,
        ["handover id=core.group.look focus=core.clock-format"]
    );
}

#[test]
fn the_search_asks_for_words_and_a_blank_answer_opens_nothing() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    world.open(&plugin, (LIST, "settings.list", 1), &[]);
    // The search is the first row of the main page.
    let done = press(&plugin, LIST, OPEN);
    assert_eq!(done.status, 1, "{:?}", commands(&done));
    assert_eq!(world.lines("ask"), ["ask 0 Search |  | "]);
    *world.typed.borrow_mut() = "   ".into();
    let done = answered(&plugin, 1, (0, 0, 3));
    assert_eq!(done.status, 0);
    assert!(world.lines("handover").is_empty());
    assert!(commands(&done).is_empty());
    // Words open the results with the words trimmed.
    press(&plugin, LIST, OPEN);
    *world.typed.borrow_mut() = "  font size ".into();
    let done = answered(&plugin, 2, (0, 0, 12));
    assert!(finished(done.status), "{}", done.status);
    assert_eq!(world.lines("handover"), ["handover text=font size"]);
    assert_eq!(commands(&done), ["open settings.search"]);
}

#[test]
fn a_choice_of_more_than_two_opens_its_list() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "core.group.reading"), ("focus", "core.week-start")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert!(finished(done.status), "{}", done.status);
    // Not changed on the spot: the chooser opens in the slot of choosing.
    assert_eq!(world.lines("handover"), ["handover id=core.week-start"]);
    assert_eq!(commands(&done), ["open core.choose-value"]);
}

#[test]
fn a_choice_of_two_changes_at_once() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "core.group.look"), ("focus", "core.clock-format")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert!(finished(done.status), "{}", done.status);
    // The other of the two; the page is then redrawn (H1).
    assert_eq!(commands(&done), ["set core.clock-format=12", "redraw"]);
}

#[test]
fn a_value_chosen_in_the_list_is_kept() {
    for (key, now, kept) in [
        ("core.week-start", "core.week.sunday", "core.week.sunday"),
        ("core.week-start", "core.week.monday", "core.week.monday"),
    ] {
        let mut spec = app();
        for s in &mut spec.settings {
            if s.key == key {
                s.value = Some(V::Text(now.into()));
            }
        }
        let world = World::new(&spec, false);
        let plugin = world.load();
        world.open(&plugin, (CHOOSE, "settings.choose", 1), &[("id", key)]);
        // The focus is on the value that is set; choosing it closes the
        // list (refused here, H1: the value is sent after it, so what is
        // kept is not seen; `settings-choose-value` in `acceptance_pending`).
        let done = press(&plugin, CHOOSE, OPEN);
        assert!(finished(done.status), "{}", done.status);
        assert_eq!(commands(&done), ["close"], "{kept}");
    }
}

#[test]
fn a_choice_opened_for_no_setting_shows_nothing_to_choose() {
    for id in [
        "",
        "no.such-setting",
        "core.font-size",
        "..",
        "core.language.x",
    ] {
        let world = World::new(&app(), false);
        let plugin = world.load();
        world.open(&plugin, (CHOOSE, "settings.choose", 1), &[("id", id)]);
        let done = press(&plugin, CHOOSE, OPEN);
        assert_eq!(done.status, 0, "{id:?}");
        assert!(commands(&done).is_empty(), "{id:?}: {:?}", commands(&done));
    }
}

#[test]
fn text_typed_for_a_setting_is_kept_trimmed() {
    let world = World::new(&app(), true);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "calibre.group"), ("focus", "calibre.address")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert_eq!(done.status, 1, "{:?}", commands(&done));
    // The keyboard starts from the value it had.
    assert_eq!(world.lines("write"), ["write 192.168.1.5"]);
    assert_eq!(world.lines("ask"), ["ask 0 Address |  | "]);
    *world.typed.borrow_mut() = "  192.168.1.20:9090  ".into();
    let done = answered(&plugin, 1, (0, 0, 21));
    assert_eq!(done.status, 0);
    assert_eq!(commands(&done), ["set calibre.address=192.168.1.20:9090"]);
}

/// The look group with two themes: presets of the kind `core.theme`.
fn themed(in_use: Option<&str>) -> Spec {
    let mut spec = Spec::default();
    spec.groups = vec![G::new("core.group.look", "Appearance", 20)];
    let mut row = S::number("core.ui-row", "core.group.look", "Row", 120, (80, 240));
    let mut focus = S::text(
        "core.ui-focus",
        "core.group.look",
        "Focus",
        "core.focus.line",
    );
    // The reader's own text size, which no theme touches (first, so that a
    // reset that did not leave it would send it first).
    let size = S::number("core.ui-size", "core.group.look", "Text size", 14, (10, 30)).set();
    match in_use {
        Some("tall") => {
            row = S::number("core.ui-row", "core.group.look", "Row", 180, (80, 240)).set()
        }
        Some("framed") => {
            focus = S::text(
                "core.ui-focus",
                "core.group.look",
                "Focus",
                "core.focus.ring",
            )
            .set();
        }
        _ => {}
    }
    spec.settings = vec![size, row, focus];
    spec.presets.insert(
        "core.theme".into(),
        vec![
            ("tall".into(), "Tall".into()),
            ("framed".into(), "Framed".into()),
        ],
    );
    spec.preset_values.insert(
        "tall".into(),
        vec![
            ("core.ui-row".into(), Some(V::Int(180))),
            ("core.ui-size".into(), Some(V::Int(20))),
        ],
    );
    spec.preset_values.insert(
        "framed".into(),
        vec![(
            "core.ui-focus".into(),
            Some(V::Text("core.focus.ring".into())),
        )],
    );
    spec
}

#[test]
fn a_theme_is_chosen_at_the_top_of_the_appearance_settings() {
    // With themes the first row of the appearance page is the theme's.
    let world = World::new(&themed(None), false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "core.group.look")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert_eq!(commands(&done), ["open settings.themes"]);
    // Without themes, no such row: the first row is the first setting (a number).
    let mut spec = themed(None);
    spec.presets.clear();
    let world = World::new(&spec, false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "core.group.look")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert!(!commands(&done).contains(&"open settings.themes".to_owned()));
}

#[test]
fn none_gives_the_appearance_back_its_defaults_but_keeps_the_text_size() {
    // No theme in use: the focus is on None; choosing it resets what a theme
    // can set, never the reader's text size.
    let world = World::new(&themed(None), false);
    let plugin = world.load();
    world.open(&plugin, (THEMES, "settings.themes", 1), &[]);
    let done = press(&plugin, THEMES, OPEN);
    assert!(finished(done.status), "{}", done.status);
    // The resets are the theme's own, not the text size's.
    assert_eq!(
        commands(&done),
        ["reset core.ui-row", "reset core.ui-focus", "close"]
    );
}

#[test]
fn a_theme_in_use_has_the_focus_and_choosing_one_applies_its_preset() {
    for (theme, in_use) in [("tall", "tall"), ("framed", "framed")] {
        let world = World::new(&themed(Some(in_use)), false);
        let plugin = world.load();
        world.open(&plugin, (THEMES, "settings.themes", 1), &[]);
        let done = press(&plugin, THEMES, OPEN);
        assert!(finished(done.status), "{}", done.status);
        // The theme in use is marked and has the focus: choosing it applies
        // its preset (its text size is not a theme's: the preset names it,
        // the host leaves it, `THEME_KEPT`).
        assert_eq!(commands(&done), [format!("preset {theme}")]);
    }
}

#[test]
fn a_group_action_is_its_first_row_and_the_settings_follow() {
    let mut spec = Spec::default();
    spec.groups = vec![G::new("mailer.group", "Mail", 0)];
    spec.settings = vec![S::switch(
        "mailer.check",
        "mailer.group",
        "Check mail",
        false,
    )];
    spec.menus.insert(
        "mailer.group".into(),
        vec![M::action("mailer.open", "Open mail", 1)],
    );
    let world = World::new(&spec, false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "mailer.group")],
    );
    // The first row runs the action.
    let done = press(&plugin, GROUP, OPEN);
    assert_eq!(commands(&done), ["run mailer.open"]);
    // The setting is the second: focused by name it changes.
    let world = World::new(&spec, false);
    let plugin = world.load();
    world.open(
        &plugin,
        (GROUP, "settings.group", 1),
        &[("id", "mailer.group"), ("focus", "mailer.check")],
    );
    let done = press(&plugin, GROUP, OPEN);
    assert_eq!(commands(&done), ["set mailer.check=true", "redraw"]);
}

/// The settings open on a setting focused: the page of its group, with it.
#[test]
fn the_settings_open_on_a_setting_focused() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    world.open(
        &plugin,
        (LIST, "settings.list", 1),
        &[("focus", "core.font-size")],
    );
    // The main screen shows the page of the setting's group; its row has the
    // focus (choosing it starts changing the number: a redraw).
    let done = press(&plugin, LIST, OPEN);
    assert_eq!(commands(&done), ["redraw"], "{:?}", commands(&done));
}

#[test]
fn a_page_keeps_its_focus_when_shown_again_at_another_size() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    let at = (GROUP, "settings.group", 1);
    world.open(
        &plugin,
        at,
        &[("id", "core.group.look"), ("focus", "core.clock-format")],
    );
    // Turned and turned back: the focus stays where it was (not the first
    // row, the night mode).
    shown(&plugin, at, (800, 480));
    shown(&plugin, at, (480, 800));
    let done = press(&plugin, GROUP, OPEN);
    assert_eq!(commands(&done), ["set core.clock-format=12", "redraw"]);
    // The chooser keeps the option that has it.
    let at = (CHOOSE, "settings.choose", 2);
    let mut spec = app();
    for s in &mut spec.settings {
        if s.key == "core.week-start" {
            s.value = Some(V::Text("core.week.monday".into()));
        }
    }
    let world = World::new(&spec, false);
    let plugin = world.load();
    world.open(&plugin, at, &[("id", "core.week-start")]);
    shown(&plugin, at, (800, 480));
    let done = press(&plugin, CHOOSE, OPEN);
    assert_eq!(commands(&done), ["close"]);
}

/// What the registry was read for during `step`.
fn reads_during(world: &World, step: impl FnOnce()) -> usize {
    let before = world.reads.get();
    step();
    world.reads.get() - before
}

#[test]
fn a_covered_group_page_is_made_anew_once_it_shows() {
    let mut spec = app();
    spec.groups
        .push(G::new("core.group.reading.fonts", "Fonts", 0).within("core.group.reading"));
    let pages = [(GROUP, "settings.group", 1), (GROUP, "settings.group", 2)];
    // A page alone: what a change of a setting costs it.
    let alone = World::new(&spec, false);
    let plugin = alone.load();
    alone.open(&plugin, pages[0], &[("id", "core.group.reading")]);
    let cost = reads_during(&alone, || {
        let done = changed(&plugin, "core.group-unknown");
        assert!(finished(done.status));
    });
    assert!(cost > 0, "a page on top is made anew for a change");
    // The same page below another: not made anew for each change, but once
    // when it shows again.
    let world = World::new(&spec, false);
    let plugin = world.load();
    world.open(&plugin, pages[0], &[("id", "core.group.reading")]);
    world.open(&plugin, pages[1], &[("id", "calibre.group")]);
    let one = reads_during(&world, || {
        changed(&plugin, "core.group-unknown");
    });
    let three = reads_during(&world, || {
        for _ in 0..3 {
            changed(&plugin, "core.group-unknown");
        }
    });
    assert!(one > 0);
    // Each change makes the page on top anew, the covered one none.
    assert_eq!(three, 3 * one, "the covered page is not made anew");
    let again = reads_during(&world, || shown(&plugin, pages[0], (480, 800)));
    assert!(again > 0, "once it shows again");
    let twice = reads_during(&world, || shown(&plugin, pages[0], (480, 800)));
    assert_eq!(twice, 0, "and only once");
}

#[test]
fn a_covered_page_is_made_anew_for_a_registry_change_once_it_shows() {
    let world = World::new(&app(), false);
    let plugin = world.load();
    let below = (GROUP, "settings.group", 1);
    world.open(&plugin, below, &[("id", "core.group.reading")]);
    world.open(
        &plugin,
        (GROUP, "settings.group", 2),
        &[("id", "calibre.group")],
    );
    let done = registry_changed(&plugin);
    assert!(finished(done.status));
    assert!(reads_during(&world, || shown(&plugin, below, (480, 800))) > 0);
}

/// `core.secrets` listing `count` secrets; every list call is logged.
fn secrets(world: &World, count: u32) {
    use paperpaw_core::native_host::registrations::{Record, ServiceEnd, ServiceSecEntry};
    let log = world.log.clone();
    world.host.service("core.secrets", 1, 0, move |op, _| {
        assert_eq!(op, 2, "only the list is asked");
        log.borrow_mut().push("secrets list".to_owned());
        let mut out = Vec::new();
        for _ in 0..count {
            out.extend(
                paperpaw_scripted_host::row(&Record::ServiceSecEntry(ServiceSecEntry {
                    flags: 1,
                    plugin: "wifi",
                    name: "Home",
                    label: "Wi-Fi Home",
                }))
                .unwrap(),
            );
        }
        out.extend(
            paperpaw_scripted_host::row(&Record::ServiceEnd(ServiceEnd {
                flags: 1,
                total: count,
                generation: 0,
                next: count,
                cursor: None,
            }))
            .unwrap(),
        );
        Ok(out)
    });
}

#[test]
fn the_secrets_are_asked_of_the_service_only_for_the_page_that_lists_them() {
    // The main page lists "Passwords and keys" (its entry in the menu of
    // pages) once there are secrets: the service is asked when the page is
    // made, for the entry the menu holds.
    let world = World::new(&app(), false);
    secrets(&world, 1);
    let plugin = world.load();
    world.open(&plugin, (LIST, "settings.list", 1), &[]);
    assert_eq!(world.lines("secrets"), ["secrets list"]);
    // Without the entry in the menu, there is nothing to show or hide.
    let mut spec = app();
    spec.menus.clear();
    let world = World::new(&spec, false);
    secrets(&world, 1);
    let plugin = world.load();
    world.open(&plugin, (LIST, "settings.list", 1), &[]);
    assert!(world.lines("secrets").is_empty());
}
