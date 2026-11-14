//! The native declarations name what the interface-6 registration names: the
//! group, the nine settings (kinds, defaults, choices, titles, help, editors,
//! units; the picker as the editor of the picture and the widget over the
//! page), the screens (the picker and the sleep screen), the slot, the icon,
//! the needs and the two places of widgets, and the sleep screen's hidden
//! button hints.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
#[path = "../../test-support/declared.rs"]
mod declared;
use paperpaw_plugin_sleep::Sleep;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, MenuNeed, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("sleep"));
    Sleep.register(&mut r).unwrap();
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
                    found.push(format!(
                        "{} | {} | {:?} | {} | {:?} {:?} | {:?} unit={:?} step={} slider={}",
                        s.id.as_str(),
                        s.title,
                        s.description,
                        s.group.as_str(),
                        s.kind,
                        s.default,
                        e.editor,
                        e.unit,
                        e.step,
                        e.slider
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        groups,
        [("sleep.group".to_string(), "sleep.title".to_string(), 62)]
    );
    let ids: Vec<&str> = found
        .iter()
        .map(|f| f.split(" | ").next().unwrap())
        .collect();
    assert_eq!(
        ids,
        [
            "sleep.show",
            "sleep.folder",
            "sleep.widgets",
            "sleep.overlay-picture",
            "sleep.overlay-widget",
            "sleep.overlay-position",
            "sleep.overlay-size",
            "sleep.overlay-see-through",
            "sleep.overlay-when"
        ]
    );
    assert!(found[0].contains("Choice") && found[0].contains("Text(\"sleep.show.cat\")"));
    assert!(
        found[1].contains("Text(\"sleep\")") && found[1].contains("Slot(Id(core.choose-folder))")
    );
    assert!(found[2].contains("home.date, home.continue, stats.widget.today, home.recent"));
    assert!(found[2].contains("Slot(Id(core.edit-widgets))"));
    assert!(found[3].contains("Screen(Id(sleep.pick-overlay))"));
    assert!(found[4].contains("Screen(Id(sleep.pick-overlay))"));
    assert!(found[5].contains("Text(\"sleep.position.bottom-right\")"));
    assert!(found[6].contains("Int { min: 10, max: 100 } Int(35)"));
    assert!(found[6].contains("unit=Some(\"core.unit.percent\") step=5 slider=true"));
    assert!(found[7].contains("Int { min: 0, max: 90 } Int(0)"));
    assert!(found[8].contains("Text(\"sleep.when.always\")"));
    assert!(!batches(|d| declarations::settings(d).unwrap()).is_empty());
}

#[test]
fn screens_slot_icon_needs_and_places_match() {
    let (mut screens, mut slots, mut icons) = (Vec::new(), Vec::new(), Vec::new());
    let (mut needs, mut places) = (Vec::new(), Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::FixedScreens(list) => {
                for s in list.iter() {
                    screens.push((
                        s.id().as_str().to_string(),
                        s.title().to_string(),
                        s.how_many() == paperpaw_sdk::Instances::SINGLE,
                    ));
                }
            }
            Contribution::SlotBinding { slot, screen, .. } => {
                slots.push((slot.to_string(), screen.to_string()));
            }
            Contribution::Icon { item, icon, .. } => {
                icons.push((item.as_str().to_string(), icon.to_string()));
            }
            Contribution::MenuNeed { item, need, .. } => {
                assert_eq!(need, MenuNeed::Sleep);
                needs.push(item.as_str().to_string());
            }
            Contribution::WidgetPlace(p) => places.push((
                p.id.as_str().to_string(),
                p.settings
                    .iter()
                    .map(|s| s.as_str().to_string())
                    .collect::<Vec<_>>(),
                p.shown_if
                    .as_ref()
                    .map(|(k, v)| (k.as_str().to_string(), (*v).to_string())),
            )),
            _ => {}
        }
    }
    assert_eq!(
        screens,
        [
            (
                "sleep.pick-overlay".to_string(),
                "sleep.setting.overlay-picture".to_string(),
                false
            ),
            ("sleep.screen".to_string(), "sleep.title".to_string(), true),
        ]
    );
    assert_eq!(
        slots,
        [("core.sleep".to_string(), "sleep.screen".to_string())]
    );
    assert_eq!(icons, [("sleep.group".to_string(), "moon".to_string())]);
    assert_eq!(needs.len(), 10);
    assert_eq!(needs[0], "sleep.group");
    assert_eq!(
        places,
        [
            (
                "sleep.widgets".to_string(),
                vec!["sleep.widgets".to_string()],
                Some(("sleep.show".to_string(), "sleep.show.widgets".to_string()))
            ),
            (
                "sleep.overlay".to_string(),
                vec!["sleep.overlay-widget".to_string()],
                Some(("sleep.show".to_string(), "sleep.show.overlay".to_string()))
            ),
        ]
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert_ne!(declarations::PICK, declarations::SCREEN);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
}

/// What the batches say, decoded and checked by the host's rules, is what the
/// interface-6 registration says: every field of the group, the nine settings,
/// their extras (editors, units, steps, sliders) and the screens; the rest is
/// checked and refused as unsupported until the host takes it.
#[test]
fn the_decoded_batches_say_what_the_registration_says() {
    let mut all = Vec::new();
    for batch in [
        batches(|d| declarations::settings(d).unwrap()),
        batches(|d| declarations::screens(d).unwrap()),
        batches(|d| declarations::navigation(d).unwrap()),
    ] {
        all.extend(batch);
    }
    let native = declared::native("sleep", &all);
    let old = declared::legacy("sleep", "sleep.group", legacy());
    assert_eq!(native.groups, old.groups);
    assert_eq!(native.specs.len(), 9);
    assert_eq!(native.specs, old.specs);
    // The picker of the picture and of the widget over the page (`pick-overlay`):
    // the editor of both settings, and its screen, as in interface 6.
    assert_eq!(native.extras, old.extras);
    let picked = |rows: &[String]| rows.iter().filter(|r| r.contains("pick-overlay")).count();
    assert_eq!(picked(&native.extras), 2);
    assert_eq!(native.screens, old.screens);
    assert_eq!(native.screens.len(), 2);
    assert_eq!(picked(&native.screens), 1);
    // The navigation records, decoded: the offer, the two places of widgets
    // (their settings, the layout, the condition), the icon and the ten needs.
    assert_eq!(native.slots, old.slots);
    assert_eq!(native.slots, ["core.sleep|sleep.screen|0"]);
    assert_eq!(native.widget_places, old.widget_places);
    assert_eq!(native.widget_places.len(), 2);
    assert!(
        native
            .widget_places
            .iter()
            .all(|p| p.contains("|Stack|sleep.show="))
    );
    assert_eq!(native.icons, old.icons);
    assert_eq!(native.icons, ["sleep.group|moon"]);
    assert_eq!(native.needs, old.needs);
    assert_eq!(native.needs.len(), 10);
    assert!(native.needs.iter().all(|n| n.ends_with("|Sleep")));
    assert_eq!(native.status_places, old.status_places);
    assert_eq!(native.keys, old.keys);
    assert_eq!(native.menu_entries, old.menu_entries);
    assert_eq!(native.actions, old.actions);
    assert!(native.events.is_empty());
    assert!(native.others.is_empty(), "{:?}", native.others);
}

/// The sleep screen shows no row of button hints, as Beta 3's had none
/// (`ScreenDecl::hints_hidden`, flag bit 8); the picker keeps its hints.
#[test]
fn the_sleep_screen_hides_the_button_hints_and_the_picker_keeps_them() {
    use paperpaw_core::native_host::registrations::{Record, records_decode};
    const HINTS_HIDDEN: u32 = 1 << 8;
    let mut flags = Vec::new();
    for batch in batches(|d| declarations::screens(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            if let Record::ScreenSpec(spec) = record {
                flags.push((spec.name.to_string(), spec.flags & HINTS_HIDDEN != 0));
            }
        }
    }
    assert_eq!(
        flags,
        [
            ("pick-overlay".to_string(), false),
            ("screen".to_string(), true)
        ]
    );
}
