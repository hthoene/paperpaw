//! The screens, the action, the menu entries and the icons declared, against
//! what the interface-6 plugin registered: ids, titles, the menus and their
//! order, and the icons. (The sync screen, the widgets and the status item are
//! in later ports.)
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "only the screens and menus are compared")]
mod declarations;
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};

/// What is compared, with the plugin's prefix off the ids.
#[derive(Debug, Default, PartialEq, Eq)]
struct Parts {
    /// `(id, title)`
    screens: Vec<(String, String)>,
    /// `(id, title)`
    actions: Vec<(String, String)>,
    /// `(menu, item, order)`
    menus: Vec<(String, String, i32)>,
    /// `(item, icon)`
    icons: Vec<(String, String)>,
}

fn bare(id: &str) -> String {
    id.strip_prefix("stats.").unwrap_or(id).to_string()
}

fn interface6() -> Parts {
    let mut r = Registrar::new(Id::new_static("stats"));
    paperpaw_plugin_stats::Stats.register(&mut r).unwrap();
    let mut parts = Parts::default();
    for c in r.into_contributions() {
        match c {
            Contribution::FixedScreens(list) => {
                for s in list {
                    parts
                        .screens
                        .push((bare(s.id().as_str()), s.title().to_string()));
                }
            }
            Contribution::Action(a) if a.id.as_str() == "stats.open" => {
                parts
                    .actions
                    .push((bare(a.id.as_str()), a.title.to_string()));
            }
            Contribution::FixedMenuEntries(list) => {
                for e in list.iter().filter(|e| e.item.as_str() == "stats.open") {
                    parts.menus.push((
                        e.menu.as_str().to_string(),
                        bare(e.item.as_str()),
                        e.order.into(),
                    ));
                }
            }
            Contribution::FixedIcons(list) => {
                for (item, icon) in list {
                    parts.icons.push((bare(item.as_str()), (*icon).to_string()));
                }
            }
            _ => {}
        }
    }
    parts
}

fn declared() -> Parts {
    let mut parts = Parts::default();
    declarations::ui(|batch: &[u8]| {
        for record in records_decode(batch).unwrap().iter() {
            match record {
                Record::ScreenSpec(s) => parts.screens.push((bare(s.name), s.title.to_string())),
                Record::ActionSpec(a) => parts.actions.push((bare(a.name), a.title.to_string())),
                Record::MenuEntry(e) => {
                    parts
                        .menus
                        .push((e.menu.to_string(), bare(e.item), e.order))
                }
                Record::IconSpec(i) => parts.icons.push((bare(i.item), i.icon.to_string())),
                other => panic!("{other:?}"),
            }
        }
        Ok(())
    })
    .unwrap();
    parts
}

#[test]
fn the_screens_action_menu_entries_and_icons_are_the_ones_interface_6_registered() {
    let (mut old, mut new) = (interface6(), declared());
    // The sync screen is the sync port's.
    old.screens.retain(|(id, _)| id != "sync");
    for parts in [&mut old, &mut new] {
        parts.screens.sort();
        parts.menus.sort();
        parts.icons.sort();
    }
    assert_eq!(old.screens.len(), 4, "{old:?}");
    assert_eq!(new, old);
}
