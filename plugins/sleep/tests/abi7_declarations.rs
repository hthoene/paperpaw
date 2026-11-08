//! The native declarations name what the interface-6 registration names: the
//! group, the nine settings (kinds, defaults, choices, titles, help, editors,
//! units), the screens, the slot, the icon, the needs and the two places of
//! widgets. The hidden hints of the sleep screen are not declared yet.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
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
) -> usize {
    let mut count = 0;
    {
        let mut sink = |_: &[u8]| -> Result<(), i32> {
            count += 1;
            Ok(())
        };
        let mut d = Declarations::new(&mut sink as &mut dyn FnMut(&[u8]) -> Result<(), i32>);
        declare(&mut d);
    }
    count
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
    assert!(batches(|d| declarations::settings(d).unwrap()) >= 1);
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
    assert_eq!(batches(|d| declarations::screens(d).unwrap()), 1);
    assert!(batches(|d| declarations::navigation(d).unwrap()) >= 1);
}
