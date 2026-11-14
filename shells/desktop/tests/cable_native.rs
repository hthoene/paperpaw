//! The computer's lines over the cable reach the native transfer plugin (#900,
//! todo 591): `core.net` is wired with the cable, so `GET /api/hello` is
//! answered by the plugin that serves `net_serve` port 0 and its screen opens.

#![allow(
    clippy::unwrap_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};

/// The answer to request `n` among `lines` (status, body), if there is one.
fn answer(lines: &[String], n: u32) -> Option<(u16, String)> {
    let mut status = None;
    let mut body = Vec::new();
    for line in lines {
        let rest = line.strip_prefix("PPT/1 ")?;
        let mut words = rest.split(' ');
        if words.next()?.parse::<u32>().ok()? != n {
            continue;
        }
        match words.next()? {
            "data" => {
                let hex = words.next()?;
                body.extend(
                    (0..hex.len())
                        .step_by(2)
                        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap()),
                );
            }
            "end" => {}
            code => status = code.parse().ok(),
        }
    }
    status.map(|s| (s, String::from_utf8(body).unwrap()))
}

#[test]
fn a_computer_over_the_cable_is_answered_by_the_native_transfer() {
    paperpaw_emulator::budget::override_limit(None);
    let files = MemoryStorage::new()
        .with(".paperpaw/plugins/setup/done", "")
        .with("paperpaw.ini", "[help]\nshow = false\n");
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    device.cable_line("PPT/1 1 GET /api/hello 0");
    // The plugin answers on the passes that follow, as time passes.
    let mut lines = Vec::new();
    for _ in 0..20 {
        device.advance(100);
        device.tick_if_due();
        lines.extend(device.take_cable_lines());
        if answer(&lines, 1).is_some() {
            break;
        }
    }
    let (status, body) = answer(&lines, 1).unwrap_or_else(|| panic!("no answer: {lines:?}"));
    assert_eq!(status, 200, "{body}");
    assert!(body.contains(r#""protocol":1"#), "{body}");
    let mut screen = None;
    device.call(|kernel, _| {
        screen = kernel.active_screen().map(ToString::to_string);
        paperpaw_core::Effects::default()
    });
    assert_eq!(screen.as_deref(), Some("transfer.cable"));
}
