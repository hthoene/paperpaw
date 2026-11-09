//! The to-do list's screen, action and icon are the interface-6 ones: the
//! ids, the titles and the icon. The two differences are named below.
#![allow(clippy::unwrap_used, reason = "test assertions")]
use paperpaw_core::native_host::registrations::{Record, records_decode};
use paperpaw_plugin_widgets::Widgets;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Action, Contribution, Id, Instances, Plugin, Registrar};

#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script and the plugin read the same file"
)]
mod declarations;

/// The batches `$declare` submits.
macro_rules! batches {
    ($declare:path) => {{
        let mut out: Vec<Vec<u8>> = Vec::new();
        {
            let mut d = Declarations::new(|batch: &[u8]| {
                out.push(batch.to_vec());
                Ok(())
            });
            $declare(&mut d).unwrap();
        }
        out
    }};
}

/// Screens (id, title, instances), actions (id, title) and icons (item, icon).
type Legacy = (
    Vec<(String, String, Instances)>,
    Vec<(String, String)>,
    Vec<(String, String)>,
);

/// What interface 6 registers for the list: its screen (id, title, how many
/// may be open), its action (id, title) and the icon of the action.
fn legacy() -> Legacy {
    let mut r = Registrar::new(Id::new_static("widgets"));
    Widgets.register(&mut r).unwrap();
    let (mut screens, mut actions, mut icons) = (Vec::new(), Vec::new(), Vec::new());
    for c in r.into_contributions() {
        match c {
            Contribution::Screen(s) if s.id.as_str() == "widgets.todo.list" => {
                screens.push((s.id.as_str().to_string(), s.title.to_string(), s.instances));
            }
            Contribution::FixedActions(list) => {
                for fixed in list {
                    let a = Action::Fixed(fixed);
                    if a.id().as_str() == "widgets.todo.open" {
                        actions.push((a.id().as_str().to_string(), a.title().to_string()));
                    }
                }
            }
            Contribution::FixedIcons(list) => {
                for (item, icon) in list
                    .iter()
                    .filter(|(i, _)| i.as_str() == "widgets.todo.open")
                {
                    icons.push((item.as_str().to_string(), (*icon).to_string()));
                }
            }
            _ => {}
        }
    }
    (screens, actions, icons)
}

#[test]
fn the_screen_the_action_and_the_icon_are_the_interface_6_ones() {
    let (screens, actions, icons) = legacy();
    assert_eq!(screens.len(), 1);
    assert_eq!(actions.len(), 1);
    assert_eq!(icons.len(), 1);

    let mut native = Vec::new();
    for batch in batches!(declarations::screens)
        .into_iter()
        .chain(batches!(declarations::navigation))
    {
        for record in records_decode(&batch).unwrap().iter() {
            native.push(match record {
                Record::ScreenSpec(s) => {
                    format!("screen {} {} {}", s.name, s.title, s.flags >> 6 & 3)
                }
                Record::ActionSpec(a) => format!("action {} {}", a.name, a.title),
                Record::IconSpec(i) => format!("icon {} {}", i.item, i.icon),
                other => panic!("{other:?}"),
            });
        }
    }
    let plugin = |id: &str| id.strip_prefix("widgets.").unwrap().to_string();
    // The one difference of the screen: interface 6 opened one for each set
    // of parameters (`PerParams`), the native screen is `Single` (flag bits
    // 6-7 = 1). The action opens it without parameters, so the two behave the
    // same: at most one list is open.
    assert_eq!(screens[0].2, Instances::PER_PARAMS);
    assert_eq!(
        native,
        [
            format!("screen {} {} 1", plugin(&screens[0].0), screens[0].1),
            format!("action {} {}", plugin(&actions[0].0), actions[0].1),
            format!("icon {} {}", icons[0].0, icons[0].1),
        ]
    );
}
