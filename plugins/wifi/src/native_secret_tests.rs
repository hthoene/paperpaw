//! The own network's rules and the kept networks against the interface-6 ones
//! (the SDK's `Random`, `wifi::join_code`, `device_name::ssid`, `wifi::keep`)
//! on the same inputs, the bounds, and every allocation refused in turn.
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::cell::Cell;

use paperpaw_sdk::native::Error;
use paperpaw_sdk::random::{self, Random};
use paperpaw_sdk::{device_name, wifi};
use proptest::prelude::*;

use crate::fail_alloc::sweep;
use crate::native_known::{self as known, MAX_KNOWN, Remember};
use crate::native_own::{self as own, Drawn};
use crate::native_policy::Password;

std::thread_local! {
    /// The state of the byte source both sides read from, and which kind.
    static STATE: Cell<u64> = const { Cell::new(0) };
    static KIND: Cell<u8> = const { Cell::new(0) };
}

/// The next bytes of the source: 0 counting, 1 all ones, 2 all zeros, 3 never
/// a usable byte for the readable text, 4 pseudo-random.
fn fill(buf: &mut [u8]) {
    for byte in buf.iter_mut() {
        let state = STATE.get();
        *byte = match KIND.get() {
            0 => state as u8,
            1 => 0xff,
            2 => 0,
            3 => 252 + (state % 4) as u8,
            _ => {
                (state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .rotate_left(29)
                    >> 24) as u8
            }
        };
        STATE.set(state.wrapping_add(1));
    }
}

fn old_source() -> Random {
    Random::new(|buf| {
        fill(buf);
        true
    })
}

fn start(kind: u8) {
    KIND.set(kind);
    STATE.set(0);
}

fn draw(buf: &mut [u8]) -> Result<bool, Error> {
    fill(buf);
    Ok(true)
}

fn text(drawn: Option<Drawn>) -> Option<String> {
    drawn.map(|d| d.text().as_str().to_string())
}

#[test]
fn drawn_texts_are_the_texts_they_were() {
    for kind in 0..5 {
        for chars in [0, 1, 5, 8, 13, 15, 20, 51] {
            start(kind);
            let old = old_source().token(chars);
            start(kind);
            let new = own::token(chars, &mut draw).unwrap();
            assert_eq!(
                old,
                Some(new.text().as_str().to_string()),
                "token {kind} {chars}"
            );
            assert!(new.is_strong());
        }
        for chars in [0, 1, 5, 15, 40, 128] {
            start(kind);
            let old = old_source().readable(chars);
            start(kind);
            let new = text(own::readable(chars, &mut draw).unwrap());
            assert_eq!(old, new, "readable {kind} {chars}");
        }
    }
    // Unusable bytes give none, never a short text.
    start(1);
    assert!(own::readable(15, &mut draw).unwrap().is_none());
    // The letters are the set's.
    start(4);
    let drawn = own::readable(15, &mut draw).unwrap().unwrap();
    assert!(
        drawn
            .text()
            .as_str()
            .chars()
            .all(|c| own::READABLE.contains(c))
    );
    assert_eq!(own::READABLE, random::READABLE);
}

#[test]
fn a_weak_draw_is_told_and_a_refused_one_is_the_error() {
    let mut weak_once = 0;
    let mut weak = |buf: &mut [u8]| {
        fill(buf);
        weak_once += 1;
        Ok(weak_once != 1)
    };
    start(4);
    let drawn = own::readable(40, &mut weak).unwrap().unwrap();
    assert!(!drawn.is_strong());
    assert!(drawn.into_strong().is_none());
    let mut strong_draw = |buf: &mut [u8]| {
        fill(buf);
        Ok(true)
    };
    assert!(
        own::token(13, &mut strong_draw)
            .unwrap()
            .into_strong()
            .is_some()
    );
    let mut weak_token = |buf: &mut [u8]| {
        fill(buf);
        Ok(false)
    };
    assert!(
        own::token(13, &mut weak_token)
            .unwrap()
            .into_strong()
            .is_none()
    );
    let mut refused = |_: &mut [u8]| Err(Error::Unsupported);
    assert_eq!(
        own::token(13, &mut refused).unwrap_err(),
        Error::Unsupported
    );
    assert_eq!(
        own::readable(15, &mut refused).unwrap_err(),
        Error::Unsupported
    );
    // More than one draw gives is not asked for.
    assert_eq!(own::token(410, &mut draw).unwrap_err(), Error::Invalid);
    assert_eq!(own::readable(129, &mut draw).unwrap_err(), Error::Invalid);
}

#[test]
fn shown_and_joined_as_before() {
    for code in ["", "a", "acdef", "acdefh", "acdef hkmnp rt", "ünï cödé x"] {
        assert_eq!(own::grouped(code).unwrap(), random::grouped(code));
    }
    for (ssid, password) in [
        ("PaperPaw X4", "acdef"),
        ("a;b,c:d\\e\"f", "p;w,:\\\""),
        ("", ""),
        ("ünï", "päss"),
    ] {
        assert_eq!(
            own::join_code(ssid, password).unwrap(),
            wifi::join_code(ssid, password)
        );
    }
    for name in [
        "",
        "   ",
        "PaperPaw X4",
        " Reader ",
        &"x".repeat(40),
        &"ü".repeat(20),
        &format!("{}ü", "x".repeat(31)),
    ] {
        assert_eq!(
            own::ssid(name).unwrap(),
            device_name::ssid(name),
            "{name:?}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(std::env::var("PROPTEST_CASES").ok().and_then(|n| n.parse().ok()).unwrap_or(256)))]

    #[test]
    fn any_code_name_and_password_are_as_before(
        ssid in ".{0,40}",
        password in ".{0,70}",
        name in ".{0,60}",
    ) {
        prop_assert_eq!(own::join_code(&ssid, &password).unwrap(), wifi::join_code(&ssid, &password));
        prop_assert_eq!(own::ssid(&name).unwrap(), device_name::ssid(&name));
        prop_assert_eq!(own::grouped(&password).unwrap(), random::grouped(&password));
    }

    #[test]
    fn any_source_gives_the_text_it_gave(kind in 0u8..5, skip in 0u64..500, chars in 0usize..60) {
        KIND.set(kind);
        STATE.set(skip);
        let old = old_source().readable(chars);
        STATE.set(skip);
        prop_assert_eq!(old, text(own::readable(chars, &mut draw).unwrap()));
        STATE.set(skip);
        let old = old_source().token(chars.min(51));
        STATE.set(skip);
        let new = own::token(chars.min(51), &mut draw).unwrap().text().as_str().to_string();
        prop_assert_eq!(old, Some(new));
    }
}

#[test]
fn a_network_is_kept_by_its_name_with_its_label() {
    for (ssid, password) in [
        ("Home", "secret pass"),
        ("Café", ""),
        ("tab\tand\nbreak", "x"),
        (&"n".repeat(32), &"p".repeat(63)),
    ] {
        let kept = Remember::new(ssid, password).unwrap();
        let write = kept.write().unwrap();
        // As interface 6 kept it, under the plugin's own name.
        let old = wifi::keep(ssid, password);
        assert_eq!(write.target().name(), ssid);
        assert_eq!(write.target().plugin(), "");
        assert_eq!(write.display_label(), format!("Wi-Fi {ssid}"));
        assert_eq!(write.value(), password);
        assert!(write.is_changeable());
        assert_eq!(wifi::secret_id(ssid), format!("wifi.{ssid}"));
        drop(old);
    }
    // What cannot be a network's: nothing is kept.
    for (ssid, password) in [
        ("", "x"),
        (&"n".repeat(33), "x"),
        ("bell\u{7}", "x"),
        ("ok", &"p".repeat(64)),
    ] {
        assert_eq!(
            Remember::new(ssid, password).unwrap_err(),
            Error::Invalid,
            "{ssid:?}"
        );
    }
    assert!(!format!("{:?}", Remember::new("Home", "hunter2").unwrap()).contains("hunter2"));
}

#[test]
fn the_known_networks_are_what_the_secrets_hold() {
    let names: Vec<&str> = (0..100)
        .map(|i| if i % 2 == 0 { "even" } else { "odd" })
        .collect();
    assert_eq!(
        known::ssids_of(names.iter().copied()).unwrap().len(),
        MAX_KNOWN
    );
    let list = known::ssids_of(["Home", "Gone", "Work"]).unwrap();
    let found = known::known_with(&list, |name| {
        Ok((name != "Gone").then(|| Password::new(&format!("pw-{name}")).unwrap()))
    })
    .unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].ssid, "Home");
    assert_eq!(found[1].password.as_str(), "pw-Work");
    let error = known::known_with(&list, |_| Err(Error::Io)).unwrap_err();
    assert_eq!(error, Error::Io);
}

#[test]
fn refused_allocations_end_in_no_memory_and_recover() {
    for kind in [0, 4] {
        sweep(|| {
            start(kind);
            own::token(13, &mut draw).map(|d| d.text().as_str().len())
        });
        sweep(|| {
            start(kind);
            own::readable(15, &mut draw).map(|d| d.map(|d| d.text().as_str().len()))
        });
    }
    sweep(|| own::grouped("acdefhkmnprtwxy").map(|t| t.len()));
    sweep(|| own::join_code("a;b", "p:w\"").map(|t| t.len()));
    sweep(|| own::ssid(" PaperPaw Reader ").map(|t| t.len()));
    sweep(|| Remember::new("Home", "secret").map(|r| r.write().is_ok()));
    sweep(|| known::ssids_of(["Home", "Work"]).map(|l| l.len()));
    let list = known::ssids_of(["Home", "Work"]).unwrap();
    sweep(|| known::known_with(&list, |n| Password::new(n).map(Some)).map(|k| k.len()));
}

#[test]
fn the_own_network_is_made_of_strong_draws_only() {
    let mut strong = |buf: &mut [u8]| {
        fill(buf);
        Ok(true)
    };
    start(9);
    let own = own::Own::draw("  My Reader  ", &mut strong)
        .unwrap()
        .unwrap();
    assert_eq!(own.ssid(), "My Reader");
    assert_eq!(own.password().as_str().len(), 15);
    assert_eq!(own.token().as_str().len(), 13);
    let mut weak = |buf: &mut [u8]| {
        fill(buf);
        Ok(false)
    };
    assert!(own::Own::draw("x", &mut weak).unwrap().is_none());
    // A weak second draw (the token) takes the network away as well.
    let mut calls = 0;
    let mut late = |buf: &mut [u8]| {
        fill(buf);
        calls += 1;
        Ok(calls < 2)
    };
    assert!(own::Own::draw("x", &mut late).unwrap().is_none());
    let mut refused = |_: &mut [u8]| Err(Error::Unsupported);
    assert_eq!(
        own::Own::draw("x", &mut refused).unwrap_err(),
        Error::Unsupported
    );
}
