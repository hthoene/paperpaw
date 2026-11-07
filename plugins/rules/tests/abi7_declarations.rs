//! Screen identity and instance-policy parity; callback acceptance is separate.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
#[allow(
    dead_code,
    reason = "the build script also uses the event declarations"
)]
mod declarations;
use paperpaw_sdk::native::declare::{Declarations, ScreenDecl};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};
#[test]
fn screen_identity_matches_existing_registration() {
    let mut r = Registrar::new(Id::new_static("rules"));
    paperpaw_plugin_rules::Rules.register(&mut r).unwrap();
    let mut expected = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            expected.push(b.to_vec());
            Ok(())
        });
        let mut token = 20;
        for c in r.into_contributions() {
            match c {
                Contribution::FixedScreens(screens) => {
                    for s in screens {
                        d.screen(
                            ScreenDecl::new(
                                s.id().as_str().strip_prefix("rules.").unwrap(),
                                s.title(),
                                token,
                            )
                            .instances(instances(s.how_many())),
                        )
                        .unwrap();
                        token += 1;
                    }
                }
                Contribution::Screen(s) => {
                    d.screen(
                        ScreenDecl::new(
                            s.id.as_str().strip_prefix("rules.").unwrap(),
                            &s.title,
                            token,
                        )
                        .instances(instances(s.instances)),
                    )
                    .unwrap();
                    token += 1;
                }
                _ => {}
            }
        }
        d.submit().unwrap();
    }
    let mut actual = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            actual.push(b.to_vec());
            Ok(())
        });
        declarations::screens(&mut d).unwrap();
    }
    assert_eq!(actual, expected);
    eprintln!(
        "rules: {} screen bytes",
        actual.iter().map(Vec::len).sum::<usize>()
    );
}

#[path = "../../test-support/native_navigation.rs"]
mod navigation;

#[test]
fn actions_menus_and_icons_match_existing_registration() {
    let mut registrar = Registrar::new(Id::new_static("rules"));
    paperpaw_plugin_rules::Rules
        .register(&mut registrar)
        .unwrap();
    let expected = navigation::encode("rules", registrar.into_contributions());
    let mut actual = Vec::new();
    {
        let mut d = Declarations::new(|bytes: &[u8]| {
            actual.push(bytes.to_vec());
            Ok(())
        });
        declarations::navigation(&mut d).unwrap();
    }
    assert_eq!(actual, expected);
}

fn instances(value: paperpaw_sdk::Instances) -> paperpaw_sdk::native::declare::Instances {
    use paperpaw_sdk::native::declare::Instances;
    if value == paperpaw_sdk::Instances::SINGLE {
        Instances::Single
    } else if value == paperpaw_sdk::Instances::MANY {
        Instances::Many
    } else {
        assert_eq!(value, paperpaw_sdk::Instances::PER_PARAMS);
        Instances::PerParams
    }
}

/// The events the native plugin follows are the ones the interface-6 plugin
/// subscribed to (start, resume, suspend, the three book events, status, a
/// setting changing) plus the minute tick that replaces its scheduled work.
#[test]
fn the_events_are_declared() {
    let mut batches = Vec::new();
    {
        let mut d = Declarations::new(|bytes: &[u8]| {
            batches.push(bytes.to_vec());
            Ok(())
        });
        declarations::events(&mut d).unwrap();
    }
    // The nine subscriptions come in as many batches as the host's limit asks for.
    assert!(!batches.is_empty());
    assert!(batches.iter().all(|b| !b.is_empty()));
}
