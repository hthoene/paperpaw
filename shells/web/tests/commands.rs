//! The remote control's commands on the browser's device: screens opened,
//! settings set and read, what is shown described, the card's files, the
//! clock, as x4-remote does on an X4, answered as JSON.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use paperpaw_web::emulator::Emulator;

/// Text as the remote's lines carry it.
fn hex(text: &str) -> String {
    text.bytes().map(|b| format!("{b:02x}")).collect()
}

#[test]
#[ignore = "re-enable at native activation (#228, todo 380): the interface-6 Settings is removed (#1011) and the web emulator has no native one yet"]
fn commands_open_screens_set_settings_and_say_what_is_shown() {
    let mut x4 = Emulator::new("xteink-x4", "en", false).unwrap();
    x4.start().unwrap();
    x4.advance(5_000);
    let mut now = 5_000;
    let mut run = |x4: &mut Emulator, line: &str| {
        now += 500;
        x4.command(line, now)
    };

    // Where it is, and a tool opened by its action.
    assert_eq!(
        run(&mut x4, "screen").unwrap(),
        r#"{"screen":"home.dashboard"}"#
    );
    assert_eq!(run(&mut x4, "run core.settings").unwrap(), "{}");
    assert!(x4.shows("settings.list"), "by its id");
    assert!(x4.shows("Library"), "by what it shows");
    assert!(!x4.shows("No such words"));
    let described = run(&mut x4, "describe").unwrap();
    assert!(
        described.starts_with(r#"{"screen":"settings.list","parts":[{"role":"#),
        "{described}"
    );
    assert!(described.contains(r#""label":"Library""#), "{described}");
    assert!(described.contains(r#""state":"focused""#), "{described}");
    let screens = run(&mut x4, "screens").unwrap();
    assert!(screens.contains(r#""settings.list""#), "{screens}");

    // A slot and a screen by id, with parameters.
    assert_eq!(run(&mut x4, "slot core.home").unwrap(), "{}");
    assert!(x4.shows("home.dashboard"));
    let add = format!("show home.add-widget page={}", hex("0"));
    assert_eq!(run(&mut x4, &add).unwrap(), "{}");
    assert!(x4.shows("Add a widget"));

    // Settings: set, read one, read by prefix.
    let night = format!("set core.night-mode {}", hex("on"));
    assert_eq!(run(&mut x4, &night).unwrap(), "{}");
    assert_eq!(
        run(&mut x4, "setting core.night-mode").unwrap(),
        r#"{"value":"on"}"#
    );
    assert!(
        run(&mut x4, "settings core.night")
            .unwrap()
            .contains(r#""core.night-mode":"on""#)
    );
    let refused = format!("set core.night-mode {}", hex("sideways"));
    assert!(run(&mut x4, &refused).is_err());
    assert!(run(&mut x4, "setting no.such").is_err());

    // What runs.
    assert!(
        run(&mut x4, "actions core.settings")
            .unwrap()
            .contains(r#""core.settings""#)
    );
    let plugins = run(&mut x4, "plugins").unwrap();
    assert!(plugins.contains(r#""id":"home","on":true"#), "{plugins}");
    assert!(
        run(&mut x4, "version")
            .unwrap()
            .contains(r#""device":"xteink-x4""#)
    );

    // The clock.
    assert_eq!(run(&mut x4, "time set 1800000000000").unwrap(), "{}");
    let time = run(&mut x4, "time").unwrap();
    assert!(time.starts_with(r#"{"unix_ms":18000000"#), "{time}");

    // The card.
    assert_eq!(run(&mut x4, "mkdir notes").unwrap(), "{}");
    x4.put_file("notes", "a.txt", b"some words").unwrap();
    assert_eq!(
        run(&mut x4, "stat plugins/notes/a.txt").unwrap(),
        r#"{"kind":"f","size":10}"#
    );
    assert_eq!(
        run(&mut x4, "stat plugins/notes").unwrap(),
        r#"{"kind":"d","size":0}"#
    );
    let moved = format!(
        "mv {} {}",
        hex("plugins/notes/a.txt"),
        hex("plugins/notes/b.txt")
    );
    assert_eq!(run(&mut x4, &moved).unwrap(), "{}");
    assert!(
        run(&mut x4, "ls plugins/notes")
            .unwrap()
            .contains(r#""name":"b.txt","dir":false,"size":10"#)
    );
    assert_eq!(run(&mut x4, "rm plugins/notes").unwrap(), "{}");
    assert_eq!(
        run(&mut x4, "stat plugins/notes/b.txt").unwrap(),
        r#"{"kind":"-","size":0}"#
    );

    // Buttons as the remote presses them; what a browser has not.
    assert_eq!(run(&mut x4, "press right").unwrap(), "{}");
    assert!(run(&mut x4, "press menu").is_err(), "no such button");
    assert!(
        run(&mut x4, "shot")
            .unwrap_err()
            .contains("not in the browser")
    );
    assert!(run(&mut x4, "bogus").unwrap_err().contains("no command"));
}
