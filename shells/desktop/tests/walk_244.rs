//! The input walk (#244, safety net P3): the emulator is driven with buttons
//! (and, on a touch profile, taps) through Home, the drawer, Settings and back,
//! and `describe` prints the screen after every step. The whole output is
//! compared byte for byte with `tests/walk-244/<profile>.txt`, so that what the
//! kernel does with an input (focus, which screen opens, where parts sit) stays
//! the same while the interface-6 code is taken out of it. The clock and the
//! date are the computer's: their lines are written as `<time>` and `<date>`
//! without a place; nothing else is changed.
//!
//! After a change that is meant: `PAPERPAW_BLESS=1 cargo test -p
//! paperpaw-emulator --test walk_244` writes the goldens anew.
//!
//! Not here, and why: the card taken out (the script language has no step
//! for it), the lock (it needs a code set first; no step sets one), a plugin's
//! install confirmation (it needs the store's network), the quick panel (no
//! button of the emulator opens it); the firmware screens are P1's.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The drawer's tools in its order: each is opened, looked at and left.
const TOOLS: [&str; 9] = [
    "Library",
    "Vocabulary",
    "Flash cards",
    "News",
    "Files",
    "Catalogs",
    "Wi-Fi transfer",
    "Plugin store",
    "Settings",
];

/// Home: the focus moves, a long press, the drawer on and off.
const X4_HOME: &str = "wait:3000,describe,right,describe,right,describe,left,describe,\
    long:confirm,wait:800,describe,back,wait:500,describe,\
    run:home.all-tools,wait:1500,describe,right,describe,right,describe,\
    page-forward,describe,page-back,describe,back,wait:500,describe";

/// Settings, Display: a choice, a switch, a number and the reset with its question.
const X4_SETTINGS: &str = "wait:3000,run:home.all-tools,wait:1200,\
    right,right,right,right,right,right,right,right,confirm,wait:2000,describe,\
    right,right,right,confirm,wait:1500,describe,confirm,wait:800,describe,back,wait:500,describe,\
    right,confirm,wait:800,describe,confirm,wait:800,describe,right,describe,confirm,wait:800,describe,\
    back,wait:500,right,confirm,wait:800,describe,right,confirm,wait:800,describe,back,wait:500,\
    back,wait:500,describe,back,wait:500,describe,back,wait:500,describe";

/// A book from the library: a page on and back, the menu and its More, Home.
const X4_BOOK: &str = "wait:3000,run:home.all-tools,wait:1200,confirm,wait:2000,describe,\
    right,confirm,wait:8000,describe,page-forward,wait:1500,describe,page-back,wait:1500,describe,\
    confirm,wait:1000,describe,right,right,right,right,right,right,confirm,wait:1000,describe,\
    back,wait:500,describe,back,wait:500,describe,confirm,wait:800,\
    right,right,right,right,right,right,right,confirm,wait:2000,describe";

/// The power menu, and the device asleep and awake.
const X4_POWER: &str = "wait:3000,long:power,wait:800,describe,back,wait:500,describe,\
    power,wait:500,describe,power,wait:2000,describe";

/// Home on a touch profile: the page buttons, a tap, back.
const BOOX_HOME: &str = "wait:3000,describe,page-forward,describe,page-back,describe,\
    tap-on:Edit,wait:800,describe,back,wait:500,describe,\
    run:home.all-tools,wait:1500,describe,back,wait:500,describe";

/// Settings, Display by taps: a number, a switch, the reset with its question.
const BOOX_SETTINGS: &str = "wait:3000,run:home.all-tools,wait:1200,tap-on:Settings,wait:2000,\
    describe,tap-on:Display,wait:1500,describe,tap-on:+,wait:500,describe,\
    tap-on:More,wait:1000,describe,tap-on:Flash on screen change,wait:500,describe,\
    tap-on:Reset to defaults,wait:800,describe,tap-on:Cancel,wait:500,describe,\
    back,wait:500,back,wait:500,back,wait:500,describe";

/// A book from the library by taps: a page on and back.
const BOOX_BOOK: &str = "wait:3000,run:home.all-tools,wait:1200,tap-on:Library,wait:2000,describe,\
    tap-on:Alice,wait:8000,describe,page-forward,wait:1500,describe,page-back,wait:1500,describe,\
    back,wait:800,describe,back,wait:500,describe";

/// The walk that opens each tool of the drawer and leaves it (buttons).
fn x4_tools() -> String {
    let mut script = String::from("wait:3000");
    for (at, _) in TOOLS.iter().enumerate() {
        script.push_str(",run:core.home,wait:500,run:home.all-tools,wait:1200");
        for _ in 0..at {
            script.push_str(",right");
        }
        script.push_str(",confirm,wait:2500,describe,back,wait:600");
    }
    script
}

/// The same by taps on the labels.
fn boox_tools() -> String {
    let mut script = String::from("wait:3000");
    for tool in TOOLS {
        script.push_str(&format!(
            ",run:core.home,wait:500,run:home.all-tools,wait:1200,tap-on:{tool},wait:2500,describe,back,wait:600"
        ));
    }
    script
}

fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A fresh library with the sample books, past the setup.
fn library(profile: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-walk-{profile}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    for entry in fs::read_dir(root().join("samples/library/books")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
    }
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    dir
}

/// Whether `label` is a clock time (`10:41`).
fn is_time(label: &str) -> bool {
    let Some((hours, minutes)) = label.split_once(':') else {
        return false;
    };
    (1..=2).contains(&hours.len())
        && minutes.len() == 2
        && hours
            .bytes()
            .chain(minutes.bytes())
            .all(|b| b.is_ascii_digit())
}

/// Whether `label` is the date line (`Wednesday, 14 October`).
fn is_date(label: &str) -> bool {
    [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ]
    .iter()
    .any(|day| label.starts_with(&format!("{day}, ")))
}

/// What the run printed, the computer's clock and date taken out, and the
/// summary reduced to the screen it ends on (its heap numbers are not the walk's).
fn walk_text(stdout: &str) -> String {
    let mut text = String::new();
    for line in stdout.lines() {
        let label = line.split('"').nth(1).unwrap_or_default();
        if line.starts_with("Text \"") && is_time(label) {
            text.push_str("Text \"<time>\"\n");
        } else if line.starts_with("Text \"") && is_date(label) {
            text.push_str("Text \"<date>\"\n");
        } else if line.starts_with("Text \"http://") && label.contains("?k=") {
            text.push_str("Text \"<address with a key>\"\n");
        } else if let Some((profile, rest)) = line.split_once(": screen ") {
            let screen = rest.split(',').next().unwrap_or_default();
            text.push_str(&format!("{profile}: screen {screen}\n"));
        } else {
            text.push_str(line);
            text.push('\n');
        }
    }
    text
}

/// Runs `script` in a fresh library: what it printed, as the walk's text.
fn run(profile: &str, script: &str) -> String {
    let library = library(profile);
    let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args([
            "--profile",
            profile,
            "--headless",
            "--network",
            "fixtures",
            "--library",
        ])
        .arg(&library)
        .args(["--set", "help.show=false"])
        .args(["--script", script])
        .output()
        .unwrap();
    let _ = fs::remove_dir_all(&library);
    assert!(
        output.status.success(),
        "the walk on {profile} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    walk_text(&String::from_utf8_lossy(&output.stdout))
}

fn walk(profile: &str, scenes: &[(&str, String)]) {
    let mut shown = String::new();
    for (name, script) in scenes {
        shown.push_str(&format!("== {name}\n{}", run(profile, script)));
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/walk-244")
        .join(format!("{profile}.txt"));
    if std::env::var_os("PAPERPAW_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &shown).unwrap();
        return;
    }
    let kept = fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("{} missing: PAPERPAW_BLESS=1 makes it", path.display()))
        .replace("\r\n", "\n");
    assert_eq!(
        shown, kept,
        "the walk on {profile} shows something else now (PAPERPAW_BLESS=1 if meant)"
    );
}

#[test]
fn the_buttons_walk_the_x4_through_home_its_tools_settings_a_book_and_power() {
    walk(
        "xteink-x4",
        &[
            ("home", X4_HOME.into()),
            ("tools", x4_tools()),
            ("settings", X4_SETTINGS.into()),
            ("book", X4_BOOK.into()),
            ("power", X4_POWER.into()),
        ],
    );
}

#[test]
fn buttons_and_taps_walk_the_boox_through_home_its_tools_settings_and_a_book() {
    walk(
        "boox-go-7",
        &[
            ("home", BOOX_HOME.into()),
            ("tools", boox_tools()),
            ("settings", BOOX_SETTINGS.into()),
            ("book", BOOX_BOOK.into()),
        ],
    );
}
