//! The setup against the interface-6 screen's scenarios (`tests/phone.rs`),
//! its form reading against the old functions on the same text, and
//! refusals of allocation.
use super::page::{chosen, escape, field};
use super::*;
use crate::fail_alloc::sweep;
use alloc::string::ToString;
use alloc::{format, vec};

const OWN: &str = "Test device";

fn phone() -> Phone {
    Phone::new(Some(OWN), "tok3n", false, 100).unwrap()
}

fn hosting(guests: u32) -> Radio<'static> {
    Radio {
        on: true,
        hosting: true,
        guests,
        online: None,
    }
}

/// The phone shown, the scan back, the own network open and served.
fn open() -> Phone {
    let mut p = phone();
    assert_eq!(
        p.shown(false).unwrap(),
        [
            Effect::Policy {
                manual: true,
                phone: true
            },
            Effect::RadioOn,
            Effect::Scan
        ]
    );
    assert_eq!(p.scanned().unwrap(), [Effect::Redraw, Effect::Host]);
    assert_eq!(
        p.radio_changed(200, &hosting(0)).unwrap(),
        [Effect::Serve, Effect::WakeAt(200 + IDLE_MS), Effect::Redraw]
    );
    assert_eq!(
        p.server_started("192.168.4.1:80").unwrap(),
        [Effect::Redraw]
    );
    p
}

#[test]
fn a_network_chosen_on_the_phone_is_joined() {
    let mut p = open();
    assert_eq!(p.page_address(), Some("http://192.168.4.1/"));
    // The page lists what was found.
    let effects = p.request(300, 1, "GET", "/", None).unwrap();
    assert_eq!(
        effects,
        [
            Effect::WakeAt(300 + IDLE_MS),
            Effect::Respond {
                request: 1,
                status: 200,
                page: Page::Choose { again: false }
            }
        ]
    );
    // A form from another page, without the token: refused, nothing joins.
    let effects = p
        .request(400, 9, "POST", "/join", Some("ssid=Evil&password=x"))
        .unwrap();
    assert!(matches!(
        effects[1],
        Effect::Respond {
            status: 403,
            page: Page::Choose { again: true },
            ..
        }
    ));
    assert!(
        p.timer(2400, &hosting(0))
            .unwrap()
            .iter()
            .all(|e| matches!(e, Effect::WakeAt(_)))
    );
    // Chosen: answered first, then the own network closes for that one.
    let form = "t=tok3n&ssid=Home&other=&password=s3cret";
    let effects = p.request(500, 2, "POST", "/join", Some(form)).unwrap();
    assert_eq!(effects.len(), 4);
    assert_eq!(
        effects[1],
        Effect::Respond {
            request: 2,
            status: 200,
            page: Page::Joining("Home".to_string())
        }
    );
    assert_eq!(effects[2], Effect::WakeAt(500 + LEAVE_MS));
    assert!(matches!(p.stage(), Stage::Leaving { .. }));
    // Not before the answer went.
    let effects = p.timer(500 + LEAVE_MS, &hosting(0)).unwrap();
    assert_eq!(
        effects,
        [
            Effect::StopServing,
            Effect::Choose {
                ssid: "Home".to_string(),
                password: Password::new("s3cret").unwrap()
            },
            Effect::Redraw
        ]
    );
    assert!(matches!(p.stage(), Stage::Joining(ssid) if ssid == "Home"));
    // Online in it: connected, and the screen closes after a while.
    let online = Radio {
        on: true,
        hosting: false,
        guests: 0,
        online: Some("Home"),
    };
    let effects = p.radio_changed(4000, &online).unwrap();
    assert_eq!(effects, [Effect::WakeAt(4000 + DONE_MS), Effect::Redraw]);
    assert!(matches!(p.stage(), Stage::Joined(ssid) if ssid == "Home"));
    let effects = p.timer(4000 + DONE_MS, &online).unwrap();
    assert_eq!(
        effects,
        [
            Effect::StopServing,
            Effect::Policy {
                manual: false,
                phone: false
            },
            Effect::Back
        ]
    );
}

#[test]
fn next_goes_on_where_the_phone_does_not_say_it_joined() {
    let mut p = open();
    let radio = hosting(0);
    assert_eq!(p.step(), Step::Join);
    let step = |p: &mut Phone, action| p.action(action, &radio).unwrap();
    assert_eq!(step(&mut p, Action::Confirm), Some(vec![Effect::Redraw]));
    assert_eq!(p.step(), Step::Open);
    assert_eq!(step(&mut p, Action::PrevPage), Some(vec![Effect::Redraw]));
    assert_eq!(p.step(), Step::Join);
    assert_eq!(step(&mut p, Action::NextPage), Some(vec![Effect::Redraw]));
    // Already there: nothing to draw, still taken.
    assert_eq!(step(&mut p, Action::NextPage), Some(vec![]));
    assert_eq!(step(&mut p, Action::Confirm), Some(vec![Effect::Redraw]));
    assert_eq!(p.step(), Step::Join);
    // The first phone to join moves it on, once; one going back after that
    // stays.
    let effects = p.radio_changed(600, &hosting(1)).unwrap();
    assert_eq!(effects, [Effect::WakeAt(600 + IDLE_MS), Effect::Redraw]);
    assert_eq!(p.step(), Step::Open);
    assert_eq!(step(&mut p, Action::Confirm), Some(vec![Effect::Redraw]));
    assert!(p.radio_changed(700, &hosting(2)).unwrap().is_empty());
    assert_eq!(p.step(), Step::Join, "once only");
    // Other keys are not its.
    assert_eq!(
        step(&mut p, Action::Back).unwrap().last(),
        Some(&Effect::Back)
    );
}

#[test]
fn a_phone_there_when_the_network_opens_shows_the_page_at_once() {
    let mut p = phone();
    p.shown(true).unwrap();
    p.scanned().unwrap();
    p.radio_changed(200, &hosting(1)).unwrap();
    assert_eq!(p.step(), Step::Open);
    // Not again when it goes back.
    p.action(Action::Confirm, &hosting(1)).unwrap();
    assert!(p.radio_changed(300, &hosting(2)).unwrap().is_empty());
}

#[test]
fn a_password_not_taken_opens_the_own_network_again() {
    let mut p = open();
    let form = "t=tok3n&ssid=Home&password=wrong";
    p.request(300, 1, "POST", "/join", Some(form)).unwrap();
    p.timer(300 + LEAVE_MS, &hosting(0)).unwrap();
    let effects = p.failed(5000, "Home", "Wrong password for Home").unwrap();
    assert_eq!(effects, [Effect::Host, Effect::Redraw]);
    assert_eq!(p.problem(), Some("Wrong password for Home"));
    assert!(matches!(p.stage(), Stage::Opening));
    // A failure of some other network changes nothing.
    assert!(p.failed(5100, "Cafe", "x").unwrap().is_empty());
    assert_eq!(p.problem(), Some("Wrong password for Home"));
}

#[test]
fn the_own_network_that_cannot_open_ends_the_setup_by_itself() {
    let mut p = phone();
    p.shown(true).unwrap();
    p.scanned().unwrap();
    let effects = p.failed(300, OWN, "unused").unwrap();
    assert_eq!(effects, [Effect::WakeAt(300 + IDLE_MS), Effect::Redraw]);
    assert!(matches!(p.stage(), Stage::Failed(Failed::NotOpened)));
    // Left alone as long as the others: closed, the network not open.
    let effects = p.timer(300 + IDLE_MS, &Radio::default()).unwrap();
    assert_eq!(effects.last(), Some(&Effect::Back));
    assert!(!effects.contains(&Effect::Leave));
}

#[test]
fn the_own_network_left_alone_closes_after_ten_minutes() {
    let mut p = open();
    // Nine minutes in: the timer only wakes again at the end of the ten.
    let effects = p.timer(200 + 9 * 60 * 1000, &hosting(0)).unwrap();
    assert_eq!(effects, [Effect::WakeAt(200 + IDLE_MS)]);
    let effects = p.timer(200 + 11 * 60 * 1000, &hosting(0)).unwrap();
    assert_eq!(
        effects,
        [
            Effect::StopServing,
            Effect::Leave,
            Effect::Policy {
                manual: false,
                phone: false
            },
            Effect::Back
        ]
    );
    // A request counts as a touch.
    let mut p = open();
    p.request(300_000, 1, "GET", "/", None).unwrap();
    let effects = p.timer(200 + IDLE_MS, &hosting(0)).unwrap();
    assert_eq!(effects, [Effect::WakeAt(300_000 + IDLE_MS)]);
}

#[test]
fn without_a_password_for_the_own_network_nothing_opens() {
    let mut p = Phone::new(None, "t", false, 0).unwrap();
    assert_eq!(p.shown(false).unwrap(), [Effect::Redraw]);
    // As on interface 6: drawn again, but the own network is not opened.
    assert_eq!(p.scanned().unwrap(), [Effect::Redraw]);
    assert!(matches!(p.stage(), Stage::Searching));
}

#[test]
fn the_server_failing_shows_why() {
    let mut p = open();
    assert_eq!(p.server_failed("no room").unwrap(), [Effect::Redraw]);
    assert!(matches!(p.stage(), Stage::Failed(Failed::Server(r)) if r == "no room"));
}

#[test]
fn what_the_radio_keeps_after_the_screen_is_what_the_user_had() {
    let mut p = Phone::new(Some(OWN), "t", true, 0).unwrap();
    p.shown(true).unwrap();
    let effects = p.action(Action::Back, &Radio::default()).unwrap().unwrap();
    assert!(effects.contains(&Effect::Policy {
        manual: true,
        phone: false
    }));
}

#[test]
fn requests_are_answered_as_the_old_screen_answers_them() {
    let mut p = open();
    let status = |p: &mut Phone, method, target, form: Option<&str>| match p
        .request(300, 1, method, target, form)
        .unwrap()[1]
    {
        Effect::Respond { status, .. } => status,
        ref other => panic!("{other:?}"),
    };
    assert_eq!(status(&mut p, "GET", "/", None), 200);
    assert_eq!(status(&mut p, "GET", "/?a=b", None), 200);
    assert_eq!(status(&mut p, "GET", "/join", None), 404);
    assert_eq!(status(&mut p, "POST", "/", None), 404);
    assert_eq!(status(&mut p, "POST", "/join", None), 403);
    assert_eq!(status(&mut p, "POST", "/join", Some("t=other&ssid=a")), 403);
    assert_eq!(
        status(&mut p, "POST", "/join", Some("t=tok3n&other=&password=x")),
        400
    );
    let long = format!("t=tok3n&other={}", "n".repeat(33));
    assert_eq!(status(&mut p, "POST", "/join", Some(&long)), 400);
    // An empty token never matches.
    let mut p = Phone::new(Some(OWN), "", false, 0).unwrap();
    assert_eq!(status(&mut p, "POST", "/join", Some("t=&ssid=a")), 403);
}

/// What the interface-6 screen read of these forms (fixed when its code went):
/// the fields `t`, `ssid`, `other`, `password`, then the network chosen.
#[test]
fn the_form_is_read_as_the_old_screen_read_it() {
    type Fields<'a> = [Option<&'a str>; 4];
    type Case<'a> = (&'a str, Fields<'a>, Option<(&'a str, &'a str)>);
    let long = format!("other={}", "n".repeat(33));
    let cases: [Case<'_>; 13] = [
        (
            "ssid=Home+Net&other=&password=p%26ss+w",
            [None, Some("Home Net"), Some(""), Some("p&ss w")],
            Some(("Home Net", "p&ss w")),
        ),
        (
            "ssid=Home&other=+Cafe+&password=",
            [None, Some("Home"), Some(" Cafe "), Some("")],
            Some(("Cafe", "")),
        ),
        ("other=&password=x", [None, None, Some(""), Some("x")], None),
        (
            "t=abc&ssid=%E2%82%AC&password=%ZZ",
            [Some("abc"), Some("€"), None, Some("%ZZ")],
            Some(("€", "%ZZ")),
        ),
        (
            "ssid=a%FFb&password=",
            [None, Some("a\u{fffd}b"), None, Some("")],
            Some(("a\u{fffd}b", "")),
        ),
        (
            "ssid=Home&password=%FF",
            [None, Some("Home"), None, Some("\u{fffd}")],
            Some(("Home", "\u{fffd}")),
        ),
        (
            "ssid=Home&password=%FE%FFab%C3",
            [None, Some("Home"), None, Some("\u{fffd}\u{fffd}ab\u{fffd}")],
            Some(("Home", "\u{fffd}\u{fffd}ab\u{fffd}")),
        ),
        (
            "ssid=Home&password=%E2%82",
            [None, Some("Home"), None, Some("\u{fffd}")],
            Some(("Home", "\u{fffd}")),
        ),
        ("ssid=&password=x", [None, Some(""), None, Some("x")], None),
        ("", [None, None, None, None], None),
        (
            "noequals&ssid=x",
            [None, Some("x"), None, None],
            Some(("x", "")),
        ),
        (
            "ssid=a&ssid=b",
            [None, Some("a"), None, None],
            Some(("a", "")),
        ),
        (&long, [None, None, Some(&long[6..]), None], None),
    ];
    for (form, fields, chosen_is) in cases {
        for (name, expected) in ["t", "ssid", "other", "password"].into_iter().zip(fields) {
            assert_eq!(
                field(form, name).unwrap().as_deref(),
                expected,
                "{form} {name}"
            );
        }
        assert_eq!(field(form, "none").unwrap(), None, "{form}");
        let new = chosen(form).unwrap();
        assert_eq!(
            new.as_ref()
                .map(|(ssid, password)| (ssid.as_str(), password.as_str())),
            chosen_is,
            "{form}"
        );
    }
    assert_eq!(
        escape("a&b<c>\"d\"").unwrap(),
        "a&amp;b&lt;c&gt;&quot;d&quot;"
    );
}

#[test]
fn refused_allocations_are_errors_and_change_nothing() {
    sweep(|| {
        let mut p = Phone::new(Some(OWN), "tok3n", false, 100)?;
        p.shown(false)?;
        p.scanned()?;
        p.radio_changed(200, &hosting(0))?;
        p.server_started("192.168.4.1:80")?;
        p.request(300, 1, "GET", "/", None)?;
        p.request(
            400,
            2,
            "POST",
            "/join",
            Some("t=tok3n&ssid=Home&other=&password=s3cret"),
        )?;
        p.timer(400 + LEAVE_MS, &hosting(0))?;
        p.failed(5000, "Home", "Wrong password")?;
        p.radio_changed(5100, &hosting(0))?;
        p.action(Action::Confirm, &hosting(0))?;
        p.timer(5100 + IDLE_MS, &hosting(0))?;
        Ok(())
    });
}

#[test]
fn a_refused_allocation_leaves_the_stage_where_it_was() {
    let mut refused = 0;
    sweep(|| {
        let mut p = Phone::new(Some(OWN), "tok3n", false, 100)?;
        p.shown(false)?;
        p.scanned()?;
        p.radio_changed(200, &hosting(0))?;
        p.request(
            300,
            1,
            "POST",
            "/join",
            Some("t=tok3n&ssid=Home&password=pw"),
        )?;
        // The name is copied first: when that is refused the password is
        // still there to try again.
        let result = p.timer(300 + LEAVE_MS, &hosting(0));
        if result.is_err() {
            refused += 1;
            assert!(matches!(p.stage(), Stage::Leaving { .. }));
            assert!(p.timer(300 + LEAVE_MS, &hosting(0)).is_ok());
            assert!(matches!(p.stage(), Stage::Joining(_)));
        }
        result.map(|_| ())
    });
    assert!(refused > 0);
}

#[test]
fn a_password_that_is_no_utf8_is_read_like_the_old_screen_reads_it_and_refusals_are_swept() {
    let form = "t=tok3n&ssid=Home&password=%FF%FEab%C3";
    let (_, password) = chosen(form).unwrap().unwrap();
    assert_eq!(password.as_str(), "\u{fffd}\u{fffd}ab\u{fffd}");
    sweep(|| {
        let (ssid, password) = chosen(form)?.ok_or(Error::NotFound)?;
        assert_eq!(ssid, "Home");
        assert_eq!(password.as_str(), "\u{fffd}\u{fffd}ab\u{fffd}");
        let mut p = Phone::new(Some(OWN), "tok3n", false, 100)?;
        p.request(300, 1, "POST", "/join", Some(form))?;
        Ok(())
    });
}

#[test]
fn a_body_over_the_most_taken_counts_as_none() {
    let status = |form: &str| {
        let mut p = open();
        match p.request(300, 1, "POST", "/join", Some(form)).unwrap()[1] {
            Effect::Respond { status, .. } => status,
            ref other => panic!("{other:?}"),
        }
    };
    let head = "t=tok3n&ssid=Home&password=";
    let exact = format!("{head}{}", "p".repeat(MAX_FORM - head.len()));
    assert_eq!(exact.len(), MAX_FORM);
    assert_eq!(status(&exact), 200);
    let over = format!("{exact}p");
    assert_eq!(status(&over), 403);
}

/// With every network the host lists (strongest first, 32 at most, and more
/// than that here) the page stays within what one write of the data folder
/// takes, and it is the weakest that are left out.
#[test]
fn many_networks_make_a_page_within_the_most_and_the_weakest_go() {
    use super::page::{Found, MOST_PAGE, Texts, choose_page, choose_page_fit};
    let names: Vec<String> = (0..64)
        .map(|n| format!("A-network-with-a-long-name-{n:02}-xxxxxxxx"))
        .collect();
    let found: Vec<Found<'_>> = names
        .iter()
        .map(|ssid| Found {
            ssid,
            secured: true,
        })
        .collect();
    let texts = Texts {
        language: "en",
        title: "Set up",
        choose_one: "Choose one",
        other: "Another network",
        password: "Password",
        join: "Connect",
        note: "The reader keeps the password",
        open: "open",
    };
    let all = choose_page(&texts, &found, false, "tok3n").unwrap();
    assert!(all.len() > MOST_PAGE, "{}", all.len());
    let page = choose_page_fit(&texts, &found, false, "tok3n", MOST_PAGE).unwrap();
    assert!(page.len() <= MOST_PAGE, "{}", page.len());
    assert!(page.contains("-00-") && page.contains("<form"), "{page}");
    assert!(!page.contains("-63-"));
    // A few networks are all there.
    let few = choose_page_fit(&texts, &found[..3], false, "tok3n", MOST_PAGE).unwrap();
    assert_eq!(
        few,
        choose_page(&texts, &found[..3], false, "tok3n").unwrap()
    );
    // Allocation is refused in turn.
    sweep(|| choose_page_fit(&texts, &found, false, "tok3n", MOST_PAGE));
}
