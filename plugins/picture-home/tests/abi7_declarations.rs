//! The native declarations name what the interface-6 registration names: the
//! group and its 28 settings (kinds, defaults, levels, editors, the condition
//! of the bar's alignment), the screens, the `core.home` offer, the icon, the
//! need, the pages as a place of widgets and the bar's place of status items.
//! The screens themselves and the status items' picker are not ported yet.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_plugin_picture_home::PictureHome;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, MenuNeed, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("picture-home"));
    PictureHome.register(&mut r).unwrap();
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
            Contribution::SettingGroup(g) => {
                groups.push((g.id.as_str().to_string(), g.title.to_string(), g.order));
            }
            Contribution::FixedSettings(list) => {
                for s in list.iter() {
                    let e = s.extra();
                    found.push((
                        s.id.as_str().to_string(),
                        s.title.to_string(),
                        s.description.as_deref().map(str::to_string),
                        s.group.as_str().to_string(),
                        format!("{:?} {:?} {:?}", s.kind, s.default, s.level),
                        format!("{:?}", e.editor),
                        format!("{:?}", e.shown_if),
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        groups,
        [(
            "picture-home.group".to_string(),
            "picture-home.title".to_string(),
            221
        )]
    );
    let ids: Vec<&str> = found.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "picture-home.folder",
            "picture-home.picture",
            "picture-home.fit",
            "picture-home.anchor",
            "picture-home.zoom",
            "picture-home.lighten",
            "picture-home.brightness",
            "picture-home.contrast",
            "picture-home.plate",
            "picture-home.render",
            "picture-home.bar",
            "picture-home.bar.at",
            "picture-home.bar.shape",
            "picture-home.bar.align",
            "picture-home.bar.look",
            "picture-home.hints",
            "picture-home.bar.left-items",
            "picture-home.bar.center-items",
            "picture-home.bar.right-items",
            "picture-home.columns",
            "picture-home.rows",
            "picture-home.dots",
            "picture-home.pencil",
            "picture-home.focus-at-once",
            "picture-home.page-1",
            "picture-home.page-2",
            "picture-home.page-3",
            "picture-home.page-4",
        ]
    );
    // Every setting is in the group; its title and help follow its name.
    for f in &found {
        assert_eq!(f.3, "picture-home.group", "{}", f.0);
        let short = f.0.trim_start_matches("picture-home.");
        if short.starts_with("page-") {
            assert_eq!(f.2.as_deref(), Some("picture-home.setting.page.help"));
        } else if short.ends_with("-items") {
            assert_eq!(f.2.as_deref(), Some("picture-home.setting.items.help"));
        } else {
            let key = short.replace('.', "-");
            assert_eq!(
                f.2.as_deref(),
                Some(format!("picture-home.setting.{key}.help").as_str()),
                "{}",
                f.0
            );
        }
    }
    values_match(&found);
    bars_match(&found);
    pages_and_grid_match(&found);
    assert_eq!(
        declarations::DEFAULTS[0],
        "home.clock, home.continue, home.recent"
    );
}

type Found = (
    String,
    String,
    Option<String>,
    String,
    String,
    String,
    String,
);

fn values_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    assert_eq!(
        by("picture-home.folder").4,
        "Text Text(\"wallpapers\") Basic"
    );
    assert_eq!(
        by("picture-home.folder").5,
        "Some(Slot(Id(core.choose-folder)))"
    );
    assert_eq!(
        by("picture-home.picture").5,
        "Some(Screen(Id(picture-home.pick)))"
    );
    assert!(
        by("picture-home.fit")
            .4
            .ends_with("Text(\"picture-home.fit.fill\") Basic")
    );
    assert!(
        by("picture-home.anchor")
            .4
            .ends_with("Text(\"picture-home.anchor.center\") Basic")
    );
    assert_eq!(
        by("picture-home.zoom").4,
        "Int { min: 100, max: 300 } Int(100) Basic"
    );
    assert_eq!(
        by("picture-home.lighten").4,
        "Int { min: 0, max: 60 } Int(15) Basic"
    );
    assert!(
        by("picture-home.brightness")
            .4
            .contains("picture-home.brightness.normal\") Basic")
    );
    assert!(
        by("picture-home.contrast")
            .4
            .contains("picture-home.contrast.normal\") Basic")
    );
    assert!(by("picture-home.plate").4.ends_with(" Basic"));
    assert!(
        by("picture-home.render")
            .4
            .contains("picture-home.render.auto\") Basic")
    );
}

fn bars_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    assert!(
        by("picture-home.bar")
            .4
            .contains("picture-home.bar.on\") Basic")
    );
    assert!(
        by("picture-home.bar.at")
            .4
            .contains("picture-home.bar.top\") Basic")
    );
    assert!(
        by("picture-home.bar.shape")
            .4
            .contains("picture-home.bar.full\") Basic")
    );
    assert!(
        by("picture-home.bar.align")
            .4
            .contains("picture-home.bar.right\") Basic")
    );
    assert!(
        by("picture-home.bar.look")
            .4
            .contains("picture-home.look.normal\") Basic")
    );
    assert!(
        by("picture-home.hints")
            .4
            .contains("picture-home.bar.on\") Advanced")
    );
    assert_eq!(
        by("picture-home.bar.align").6,
        "Some((Id(picture-home.bar.shape), \"picture-home.bar.compact\"))"
    );
}

fn pages_and_grid_match(found: &[Found]) {
    let by = |id: &str| found.iter().find(|f| f.0 == id).unwrap();
    for n in 1..=4 {
        let page = by(&format!("picture-home.page-{n}"));
        assert!(page.4.ends_with("Advanced"), "{n}");
        assert_eq!(page.5, "Some(Screen(Id(picture-home.dashboard)))");
    }
    assert_eq!(
        by("picture-home.page-1").4,
        "Text Text(\"home.clock, home.continue, home.recent\") Advanced"
    );
    assert_eq!(
        by("picture-home.page-2").4,
        "Text Text(\"home.tools\") Advanced"
    );
    assert_eq!(
        by("picture-home.columns").4,
        "Int { min: 2, max: 6 } Int(3) Basic"
    );
    assert_eq!(
        by("picture-home.rows").4,
        "Int { min: 0, max: 12 } Int(0) Basic"
    );
    assert_eq!(by("picture-home.dots").4, "Bool Bool(true) Basic");
    assert_eq!(by("picture-home.pencil").4, "Bool Bool(true) Basic");
    assert_eq!(by("picture-home.focus-at-once").4, "Bool Bool(false) Basic");
    assert_eq!(
        by("picture-home.bar.left-items").4,
        "List Text(\"core.status.clock\") Basic"
    );
    assert_eq!(
        by("picture-home.bar.center-items").4,
        "List Text(\"\") Basic"
    );
    assert!(
        by("picture-home.bar.right-items")
            .4
            .contains("wifi.status, power.status")
    );
    // The titles are the keys the legacy registration has.
    assert_eq!(by("picture-home.fit").1, "picture-home.setting.fit");
    assert_eq!(
        by("picture-home.bar.left-items").1,
        "picture-home.setting.bar-left"
    );
    assert_eq!(
        by("picture-home.focus-at-once").1,
        "picture-home.setting.focus-at-once"
    );
}

#[test]
fn screens_offer_places_and_need_match() {
    let (mut screens, mut slots, mut icons) = (Vec::new(), Vec::new(), Vec::new());
    let (mut places, mut statuses, mut needs) = (Vec::new(), Vec::new(), Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((s.id().as_str().to_string(), s.title().to_string()));
                }
            }
            Contribution::SlotBinding {
                slot,
                screen,
                priority,
                ..
            } => slots.push((slot.to_string(), screen.to_string(), priority.0)),
            Contribution::Icon { item, icon, .. } => {
                icons.push((item.as_str().to_string(), icon.to_string()));
            }
            Contribution::WidgetPlace(p) => places.push((
                p.id.as_str().to_string(),
                p.title.to_string(),
                p.settings
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect::<Vec<_>>(),
            )),
            Contribution::StatusPlace(p) => statuses.push((
                p.id.as_str().to_string(),
                p.title.to_string(),
                p.parts
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
            pair("picture-home.dashboard", "picture-home.title"),
            pair("picture-home.add", "picture-home.add"),
            pair("picture-home.pick", "picture-home.setting.picture"),
        ]
    );
    assert_eq!(
        slots,
        [(
            "core.home".to_string(),
            "picture-home.dashboard".to_string(),
            declarations::PREFERRED as i16
        )]
    );
    assert!(icons.contains(&pair("picture-home.group", "layout-grid")));
    assert_eq!(places.len(), 1);
    assert_eq!(places[0].0, "picture-home.pages");
    assert_eq!(places[0].1, "picture-home.title");
    assert_eq!(
        places[0].2,
        [
            "picture-home.page-1",
            "picture-home.page-2",
            "picture-home.page-3",
            "picture-home.page-4"
        ]
    );
    assert_eq!(
        statuses,
        [(
            "picture-home.bar".to_string(),
            "picture-home.setting.bar".to_string(),
            vec![
                "picture-home.bar.left-items".to_string(),
                "picture-home.bar.center-items".to_string(),
                "picture-home.bar.right-items".to_string(),
            ]
        )]
    );
    assert_eq!(
        needs,
        [("picture-home.focus-at-once".to_string(), MenuNeed::NoTouch)]
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 1);
}

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration says: every field of the group, the 28 settings,
/// their extras (editors, the condition of the bar's alignment) and the
/// screens; the rest is checked and refused as unsupported until the host
/// takes it.
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
    let native = declared::native("picture-home", &all);
    let old = declared::legacy("picture-home", "picture-home.group", legacy());
    assert_eq!(native.groups, old.groups);
    assert_eq!(native.specs.len(), 28);
    assert_eq!(native.specs, old.specs);
    // The bar's items are picked on the host's screen in interface 6
    // (`core.status-items`); Kernel's ruling (todo 121) gives them one of the
    // plugin's own, built on the kit's `status_items`, named as a screen of the
    // plugin. Those three extras and that screen are the only differences.
    let (items, others): (Vec<_>, Vec<_>) = old
        .extras
        .iter()
        .cloned()
        .partition(|e| e.contains("-items|"));
    assert_eq!(items.len(), 3, "{items:?}");
    assert!(items.iter().all(|e| e.contains("|1:core.status-items|")));
    let (own, native_others): (Vec<_>, Vec<_>) = native
        .extras
        .iter()
        .cloned()
        .partition(|e| e.contains("-items|"));
    assert_eq!(own.len(), 3, "{own:?}");
    assert!(own.iter().all(|e| e.contains("|1:status-items|")));
    for (own, old) in own.iter().zip(&items) {
        assert_eq!(own.replace("1:status-items", "1:core.status-items"), *old);
    }
    assert_eq!(native_others, others);
    let (screen, screens): (Vec<_>, Vec<_>) = native
        .screens
        .iter()
        .cloned()
        .partition(|s| s.starts_with("status-items|"));
    assert_eq!(screen, ["status-items|picture-home.setting.bar"]);
    assert_eq!(screens, old.screens);
    // The navigation records, decoded: the offer at the replacing priority, the
    // pages as a place of widgets (their settings, the layout), the bar's place
    // of status items, the icon and the need.
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots, ["core.home|picture-home.dashboard|100"]);
    assert_eq!(native.widget_places, old.widget_places);
    assert_eq!(native.widget_places.len(), 1);
    assert!(native.widget_places[0].contains("picture-home.page-4|Grid"));
    assert_eq!(native.status_places, old.status_places);
    assert_eq!(native.status_places.len(), 1);
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons, ["picture-home.group|layout-grid"]);
    assert_eq!(native.needs, old.needs);
    assert_eq!(native.needs, ["picture-home.focus-at-once|NoTouch"]);
    assert_eq!(native.keys, old.keys);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.actions, old.actions);
    // The minute tick (the status bar) and a widget that changed
    // (`event.widget_changed`, its cell), in any order, and nothing else.
    assert_eq!(native.events.len(), 2, "{:?}", native.events);
    for token in [declarations::TICK, declarations::WIDGET_CHANGED] {
        assert!(
            native
                .events
                .iter()
                .any(|e| e.ends_with(&format!(":{token}"))),
            "{token} in {:?}",
            native.events
        );
    }
    assert!(native.others.is_empty(), "{:?}", native.others);
}
