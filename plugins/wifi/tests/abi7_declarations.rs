//! The native declarations name what the interface-6 registration names: the
//! group and its settings, the screens, the action, the status item, the
//! icons, the needs and the first-run step. Callbacks and the radio's own
//! events are a later slice (`PORTING-7.md`).
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(dead_code, reason = "tokens the plugin uses later")]
mod declarations;
use paperpaw_plugin_wifi::Wifi;
use paperpaw_sdk::native::declare::Declarations;
use paperpaw_sdk::{Contribution, Id, MenuNeed, Plugin, Registrar};

fn legacy() -> Vec<Contribution> {
    let mut r = Registrar::new(Id::new_static("wifi"));
    Wifi.register(&mut r).unwrap();
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
    let mut settings = Vec::new();
    for contribution in legacy() {
        match contribution {
            Contribution::SettingGroup(group) => groups.push((
                group.id.as_str().to_string(),
                group.title.to_string(),
                group.order,
                group.page.as_ref().map(|p| p.as_str().to_string()),
            )),
            Contribution::FixedSettings(list) => {
                for spec in list.iter() {
                    settings.push(format!(
                        "{} {} {:?} {} {:?} {:?}",
                        spec.id.as_str(),
                        spec.title,
                        spec.description,
                        spec.group.as_str(),
                        spec.default,
                        spec.kind,
                    ));
                }
            }
            _ => {}
        }
    }
    assert_eq!(
        groups,
        [(
            "wifi.group".to_string(),
            "wifi.title".to_string(),
            85,
            Some("wifi.page".to_string())
        )]
    );
    let idle = declarations::IDLE_OFF;
    assert_eq!(settings.len(), 4);
    for (setting, text) in [
        ("wifi.on-demand", "Bool(true)"),
        ("wifi.idle-off", "Text(\"wifi.idle-off.1m\")"),
        ("wifi.while-charging", "Bool(false)"),
        ("wifi.join-first", "Text(\"wifi.join-first.picked\")"),
    ]
    .iter()
    .zip(&settings)
    {
        assert!(text.starts_with(&format!("{} ", setting.0)), "{text}");
        assert!(text.contains(setting.1), "{text}");
        assert!(text.contains(" wifi.group "), "{text}");
        assert!(
            text.contains(&format!("\"{}.help\"", text.split(' ').nth(1).unwrap())),
            "{text}"
        );
    }
    for id in idle {
        assert!(settings[1].contains(id), "{id}");
    }
    assert!(settings[3].contains("wifi.join-first.strongest"));
    assert_eq!(batches(|d| declarations::settings(d).unwrap()).len(), 1);
}

#[test]
fn screens_action_status_item_icons_needs_and_step_match() {
    let mut screens = Vec::new();
    let (mut actions, mut icons, mut needs) = (Vec::new(), Vec::new(), Vec::new());
    let (mut status, mut steps) = (Vec::new(), Vec::new());
    for contribution in legacy() {
        match contribution {
            Contribution::Screen(s) => screens.push((
                s.id.as_str().to_string(),
                s.title.to_string(),
                s.instances == paperpaw_sdk::Instances::SINGLE,
            )),
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
            Contribution::MenuNeed { item, need, .. } => {
                assert_eq!(need, MenuNeed::Wifi);
                needs.push(item.as_str().to_string());
            }
            Contribution::StatusItem(s) => status.push((
                s.id.as_str().to_string(),
                s.title.to_string(),
                s.tap.as_ref().map(|t| t.as_str().to_string()),
            )),
            Contribution::SetupStep(s) => {
                steps.push((
                    s.id.as_str().to_string(),
                    s.title.to_string(),
                    s.help.to_string(),
                    s.order,
                ));
            }
            _ => {}
        }
    }
    let s = |id: &str, title: &str| (id.to_string(), title.to_string(), true);
    assert_eq!(
        screens,
        [
            s("wifi.page", "wifi.title"),
            s("wifi.networks", "wifi.networks.title"),
            s("wifi.phone", "wifi.phone.title"),
        ]
    );
    assert_eq!(
        actions,
        [("wifi.open".to_string(), "wifi.title".to_string())]
    );
    assert_eq!(
        icons,
        [
            ("wifi.group".to_string(), "wifi".to_string()),
            ("wifi.open".to_string(), "wifi".to_string())
        ]
    );
    needs.sort();
    assert_eq!(
        needs,
        [
            "wifi.idle-off",
            "wifi.join-first",
            "wifi.on-demand",
            "wifi.while-charging"
        ]
    );
    assert_eq!(
        status,
        [(
            "wifi.status".to_string(),
            "wifi.status.item".to_string(),
            Some("wifi.open".to_string())
        )]
    );
    assert_eq!(
        steps,
        [(
            "wifi.setup".to_string(),
            "wifi.title".to_string(),
            "wifi.setup.help".to_string(),
            50
        )]
    );
    assert_eq!(batches(|d| declarations::screens(d).unwrap()).len(), 1);
    assert!(!batches(|d| declarations::navigation(d).unwrap()).is_empty());
    // Twelve subscriptions (the radio's four, the timers' work and the stack's change among them) in two batches.
    assert_eq!(batches(|d| declarations::events(d).unwrap()).len(), 2);
}

/// The subscriptions, decoded: the radio's own events 92-95 go to their
/// callbacks, with the four of the device's the plugin had before.
#[test]
fn the_decoded_subscriptions_name_every_event_the_plugin_handles() {
    use paperpaw_core::native_host::registrations::{Record, Registration, records_decode};
    let mut found = Vec::new();
    for batch in batches(|d| declarations::events(d).unwrap()) {
        for record in records_decode(&batch).unwrap().iter() {
            match record {
                Record::Registration(Registration::Event { event_tag, token }) => {
                    found.push((event_tag, token));
                }
                other => panic!("{other:?}"),
            }
        }
    }
    // The device's four (not `screen_shown`: 38 goes to the declared screens' tokens, `screens-input.md` 2), then the radio's four: scanned 92, done 93,
    // network wanted 94 and idle 95 (`docs/abi/device.md`), then the work's and the served page's (server, request),
    // then the stack's change (81: any screen on top, back home or in a book the radio goes off).
    let tokens: Vec<u32> = found.iter().map(|(_, token)| *token).collect();
    assert_eq!(
        tokens,
        [
            declarations::STARTED,
            declarations::STATUS_CHANGED,
            declarations::WIFI_CHANGED,
            declarations::POWER_CHANGED,
            declarations::WIFI_SCANNED,
            declarations::WIFI_DONE,
            declarations::NETWORK_WANTED,
            declarations::NETWORK_IDLE,
            declarations::WORK,
            declarations::SERVER_EVENT,
            declarations::SERVED_REQUEST,
            declarations::STACK_CHANGED,
        ]
    );
    let radio: Vec<u32> = found[4..8].iter().map(|(tag, _)| *tag).collect();
    assert_eq!(radio, [92, 93, 94, 95]);
    // The timers (idle off, retry) come due as work (`event.work_due`).
    assert_eq!(found[8].0, 103, "work_due");
    assert_eq!(found[0].0, 32, "started");
    assert_eq!(found[11].0, 81, "stack_changed");
}
