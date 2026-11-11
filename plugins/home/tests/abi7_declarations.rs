//! The native declarations name what the interface-6 registration names for
//! what is ported: the group and its 24 settings (kinds, defaults, levels,
//! the editor of a page), the screens, the slot, the actions with their
//! icons and menu places, the swipe up, the pages as a place of widgets and
//! the need. The built-in widgets are declared by `widgets_declarations.rs`; the help
//! hints are not declared yet.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_plugin_home::Home;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, MenuNeed, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("home"));
    Home.register(&mut r).unwrap();
    r.into_contributions()
}

fn batches(
    declare: impl FnOnce(&mut Declarations<&mut dyn FnMut(&[u8]) -> Result<(), i32>>),
) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut sink = |b: &[u8]| -> Result<(), i32> {
            out.push(b.to_vec());
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    out
}

#[test]
fn the_group_and_the_settings_match() {
    let mut groups = Vec::new();
    let mut found = Vec::new();
    for contribution in legacy() {
        match contribution {
            Contribution::SettingGroup(g) if g.id.as_str() == "home.group" => {
                groups.push((g.title.to_string(), g.order, g.page.is_none()));
            }
            Contribution::FixedSettings(list) => {
                for s in list.iter().filter(|s| s.id.as_str().starts_with("home.")) {
                    // The built-in widgets' settings are `home.<widget>.…`
                    // in groups of their own.
                    if s.group.as_str() != "home.group" {
                        continue;
                    }
                    found.push((
                        s.id.as_str().to_string(),
                        s.title.to_string(),
                        s.description.as_deref().map(str::to_string),
                        format!("{:?} {:?} {:?}", s.kind, s.default, s.level),
                        format!("{:?}", s.extra().editor),
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(groups, [("home.section".to_string(), 220, true)]);
    assert_eq!(found.len(), 24, "{found:#?}");
    // Every setting of the group is declared once, in this order.
    let ids: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    for n in 1..=8 {
        assert!(ids.contains(&format!("home.page-{n}").as_str()));
        assert!(ids.contains(&format!("home.page-{n}.wide").as_str()));
    }
    for id in [
        "home.columns",
        "home.rows",
        "home.tool-labels",
        "home.focus-at-once",
        "home.status-bar",
        "home.footer.pencil",
        "home.footer.dots",
        "home.footer.items",
    ] {
        assert!(ids.contains(&id), "{id}");
    }
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    assert_eq!(
        by("home.page-1").3,
        "Text Text(\"home.clock, home.continue, home.recent\") Advanced"
    );
    assert_eq!(by("home.page-2").3, "Text Text(\"home.tools\") Advanced");
    assert_eq!(
        by("home.page-1.wide").2.as_deref(),
        Some("home.setting.page-wide.help")
    );
    assert!(by("home.page-3").4.contains("home.dashboard"));
    assert_eq!(by("home.columns").3, "Int { min: 2, max: 6 } Int(3) Basic");
    assert_eq!(by("home.rows").3, "Int { min: 0, max: 12 } Int(0) Basic");
    assert_eq!(by("home.footer.items").3, "List Text(\"\") Basic");
    assert_eq!(
        by("home.footer.items").4,
        "Some(Screen(Id(core.status-items)))"
    );
    assert!(!batches(|d| declarations::settings(d).unwrap()).is_empty());
    assert_eq!(
        declarations::DEFAULTS[0],
        "home.clock, home.continue, home.recent"
    );
}

#[test]
fn screens_slot_actions_and_places_match() {
    let (mut screens, mut slots, mut actions) = (Vec::new(), Vec::new(), Vec::new());
    let (mut icons, mut entries, mut keymaps) = (Vec::new(), Vec::new(), Vec::new());
    let (mut places, mut needs) = (Vec::new(), Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
            }
            Contribution::SlotBinding { slot, screen, .. } => {
                slots.push((slot.to_string(), screen.to_string()));
            }
            Contribution::FixedActions(list) => {
                for a in list.iter() {
                    let a = paperpaw_sdk::Action::Fixed(a);
                    actions.push((a.id().as_str().to_string(), a.title().to_string()));
                }
            }
            Contribution::FixedIcons(list) => {
                for (item, icon) in list.iter() {
                    icons.push((item.as_str().to_string(), (*icon).to_string()));
                }
            }
            Contribution::Icon { item, icon, .. } => {
                icons.push((item.as_str().to_string(), icon.to_string()));
            }
            Contribution::FixedMenuEntries(list) => {
                for e in list.iter() {
                    entries.push((
                        e.menu.as_str().to_string(),
                        e.item.as_str().to_string(),
                        e.order,
                    ));
                }
            }
            Contribution::Keymap(text) => keymaps.push(text.to_string()),
            Contribution::WidgetPlace(p) => places.push((
                p.id.as_str().to_string(),
                p.title.to_string(),
                p.settings
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect::<Vec<_>>(),
            )),
            Contribution::MenuNeed { item, need, .. } => {
                needs.push((item.as_str().to_string(), need));
            }
            _ => {}
        }
    }
    let pair = |a: &str, b: &str| (a.to_string(), b.to_string());
    assert_eq!(
        screens,
        [
            pair("home.dashboard", "home.title"),
            pair("home.add-widget", "home.edit.add"),
            pair("home.drawer", "home.drawer"),
        ]
    );
    assert_eq!(slots, [pair("core.home", "home.dashboard")]);
    assert_eq!(
        actions,
        [
            pair("home.all-tools", "home.drawer"),
            pair("home.edit-pages", "home.edit-pages")
        ]
    );
    for icon in [
        pair("home.group", "home"),
        pair("home.all-tools", "apps"),
        pair("home.edit-pages", "pencil"),
    ] {
        assert!(icons.contains(&icon), "{icon:?}");
    }
    assert_eq!(
        entries,
        [
            ("core.main".to_string(), "home.edit-pages".to_string(), 40),
            ("home.actions".to_string(), "core.library".to_string(), 10),
        ]
    );
    assert!(
        keymaps
            .iter()
            .any(|k| k.contains("[keymap.core.home]") && k.contains("swipe.up = home.all-tools"))
    );
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].0, "home.pages");
    assert_eq!(places[0].1, "home.title");
    assert_eq!(places[0].2.len(), 8);
    assert_eq!(
        needs,
        [("home.focus-at-once".to_string(), MenuNeed::NoTouch)]
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration says: every field of the group, the settings, their
/// extras (the page editors) and the screens; the rest is checked and refused
/// as unsupported until the host takes it.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = Vec::new();
    for batch in [
        batches(|d| declarations::settings(d).unwrap()),
        batches(|d| declarations::screens(d).unwrap()),
        batches(|d| declarations::navigation(d).unwrap()),
        batches(|d| declarations::events(d).unwrap()),
    ] {
        all.extend(batch);
    }
    let native = declared::native("home", &all);
    let old = declared::legacy("home", "home.group", legacy());
    assert_eq!(native.groups, old.groups);
    assert_eq!(native.specs.len(), 24);
    assert_eq!(native.specs, old.specs);
    // The footer items' picker is the host's screen (`core.status-items`) in
    // interface 6; Kernel's ruling (todo 121) gives it a screen of home's own,
    // built on the kit's `status_items`, named as a screen of the plugin. That
    // setting's extra and that screen are the only differences; every other
    // editor and screen is the same.
    let (footer, others): (Vec<_>, Vec<_>) = old
        .extras
        .iter()
        .cloned()
        .partition(|e| e.starts_with("footer.items|"));
    assert_eq!(footer, ["footer.items||||=|1:core.status-items|0|0"]);
    let (own, native_others): (Vec<_>, Vec<_>) = native
        .extras
        .iter()
        .cloned()
        .partition(|e| e.starts_with("footer.items|"));
    assert_eq!(own, ["footer.items||||=|1:status-items|0|0"]);
    assert_eq!(native_others, others);
    let (screen, screens): (Vec<_>, Vec<_>) = native
        .screens
        .iter()
        .cloned()
        .partition(|s| s.starts_with("status-items|"));
    assert_eq!(screen, ["status-items|home.setting.footer-items"]);
    assert_eq!(screens, old.screens);
    // The navigation records, decoded: the offer (slot, priority), the pages
    // as a place of widgets (their settings, the layout), the actions, the menu
    // places (menu, item, order), the icons and the key; and the event.
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots, ["core.home|home.dashboard|0"]);
    assert_eq!(native.widget_places, old.widget_places);
    assert_eq!(native.widget_places.len(), 1);
    // The status line widget's place belongs to the built-in widgets, which are
    // not declared yet (an ignored acceptance case).
    assert!(native.status_places.is_empty());
    assert_eq!(old.status_places.len(), 1, "{:?}", old.status_places);
    assert_eq!(native.keys, old.keys);
    assert_eq!(native.keys, ["core.home|swipe.up|home.all-tools"]);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.menu_entries.len(), 2);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.needs, old.needs);
    assert_eq!(native.needs, ["home.focus-at-once|NoTouch"]);
    assert_eq!(native.actions, old.actions);
    assert_eq!(native.actions.len(), 2);
    // The setting changes (the grid), the minute tick (the time shown) and a
    // widget that changed (`event.widget_changed`), in any order.
    assert_eq!(native.events.len(), 3, "{:?}", native.events);
    for token in [
        declarations::SETTING_CHANGED,
        declarations::TICK,
        declarations::WIDGET_CHANGED,
    ] {
        assert!(
            native
                .events
                .iter()
                .any(|e| e.ends_with(&format!(":{token}"))),
            "{token} in {:?}",
            native.events
        );
    }
    // Nothing else was declared.
    assert!(native.others.is_empty(), "{:?}", native.others);
}

#[test]
fn the_dashboards_help_hints_are_declared_and_worded_in_both_languages() {
    // The nine hints of interface 6 (five for touch, four for buttons), in batches
    // the host admits.
    let batches = batches(|d| declarations::help(d).unwrap());
    assert!(!batches.is_empty());
    for key in [
        "quick", "back", "home", "tools", "arrange", "move", "select", "menu", "panel",
    ] {
        let key = format!("home.tips.{key}");
        assert!(
            batches
                .iter()
                .any(|b| b.windows(key.len()).any(|w| w == key.as_bytes())),
            "{key} declared"
        );
    }
    for text in [
        include_str!("../i18n/en.ini"),
        include_str!("../i18n/de.ini"),
    ] {
        for key in [
            "quick", "back", "home", "tools", "arrange", "move", "select", "menu", "panel",
        ] {
            let key = format!("home.tips.{key} =");
            assert!(text.lines().any(|l| l.starts_with(&key)), "{key}");
        }
    }
}
