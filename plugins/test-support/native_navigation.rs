//! Test-only comparison with the original registrations, never a runtime adapter.
use paperpaw_sdk::Contribution;
use paperpaw_sdk::native::declare::{ActionDecl, Declarations, MenuEntryDecl};

pub fn encode(plugin: &str, contributions: Vec<Contribution>) -> Vec<Vec<u8>> {
    let prefix = format!("{plugin}.");
    let mut batches = Vec::new();
    {
        let mut d = Declarations::new(|bytes: &[u8]| {
            batches.push(bytes.to_vec());
            Ok(())
        });
        let mut token = 1;
        for contribution in contributions {
            match contribution {
                Contribution::FixedActions(actions) => {
                    for action in actions {
                        let action = paperpaw_sdk::Action::Fixed(action);
                        d.action(ActionDecl::new(
                            action.id().as_str().strip_prefix(&prefix).unwrap(),
                            action.title(),
                            token,
                        ))
                        .unwrap();
                        token += 1;
                    }
                }
                Contribution::FixedMenuEntries(entries) => {
                    for entry in entries {
                        assert!(
                            entry.place.is_none(),
                            "add placement mapping before accepting it"
                        );
                        let mut native =
                            MenuEntryDecl::new(entry.menu.as_str(), entry.item.as_str());
                        native.order = entry.order;
                        d.menu_entry(native).unwrap();
                    }
                }
                Contribution::FixedIcons(icons) => {
                    for (item, icon) in icons {
                        d.icon(item.as_str(), icon).unwrap();
                    }
                }
                Contribution::Action(_)
                | Contribution::MenuEntry(_)
                | Contribution::Icon { .. } => {
                    panic!("new dynamic declaration requires parity coverage");
                }
                _ => {}
            }
        }
        d.submit().unwrap();
    }
    batches
}
