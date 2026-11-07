//! Screen identity parity only; not callback or instance-policy parity.
#![allow(clippy::unwrap_used, reason = "test assertions")]
#[path = "../declarations.rs"]
mod declarations;
use paperpaw_sdk::native::declare::{Declarations, ScreenDecl};
use paperpaw_sdk::{Contribution, Id, Plugin, Registrar};
#[test]
fn screen_identity_matches_existing_registration() {
    let mut r = Registrar::new(Id::new_static("reading-goal"));
    paperpaw_plugin_reading_goal::ReadingGoal
        .register(&mut r)
        .unwrap();
    let mut expected = Vec::new();
    {
        let mut d = Declarations::new(|b: &[u8]| {
            expected.push(b.to_vec());
            Ok(())
        });
        let mut token = 10;
        for c in r.into_contributions() {
            match c {
                Contribution::FixedScreens(screens) => {
                    for s in screens {
                        d.screen(ScreenDecl::new(
                            s.id().as_str().strip_prefix("reading-goal.").unwrap(),
                            s.title(),
                            token,
                        ))
                        .unwrap();
                        token += 1;
                    }
                }
                Contribution::Screen(s) => {
                    d.screen(ScreenDecl::new(
                        s.id.as_str().strip_prefix("reading-goal.").unwrap(),
                        &s.title,
                        token,
                    ))
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
        "reading-goal: {} screen bytes",
        actual.iter().map(Vec::len).sum::<usize>()
    );
}
