//! The emulator run headless, as CI and bug reports run it: its command
//! line, its script steps (and what a wrong one says), and what the
//! emulated device does beyond drawing: sleeping until the power button
//! wakes it, turning its screen, restarting when a plugin asks, speaking
//! a book aloud.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The repository: two folders above this crate.
fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A fresh library named `name` with the sample books, past the setup.
fn library(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-headless-{name}-{}", std::process::id()));
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

/// What a run printed, and how it ended.
struct Run {
    ok: bool,
    stdout: String,
    stderr: String,
}

impl Run {
    /// The screen shown at the end, from the summary line.
    fn screen(&self) -> &str {
        let line = self.stdout.lines().last().unwrap_or_default();
        line.split("screen ")
            .nth(1)
            .and_then(|s| s.split(',').next())
            .unwrap_or_default()
    }
}

impl From<Output> for Run {
    fn from(output: Output) -> Self {
        Self {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }
}

fn emulator(args: &[&str]) -> Run {
    Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(args)
        .output()
        .unwrap()
        .into()
}

/// Files under `dir`, all levels.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return found;
    };
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            found.extend(files_under(&path));
        } else {
            found.push(path);
        }
    }
    found
}

/// A run that ends on a wrong argument writes nothing into the library it
/// would have used (the default one, `samples/library`, is relative to the
/// folder the emulator runs in: a test run must not fill the checkout with
/// the English hyphenation pack).
#[test]
fn a_wrong_argument_leaves_the_default_library_alone() {
    let cwd = std::env::temp_dir().join(format!("paperpaw-headless-cwd-{}", std::process::id()));
    let _ = fs::remove_dir_all(&cwd);
    fs::create_dir_all(&cwd).unwrap();
    for args in [
        &["--heap-report"][..],
        &["--script", "confirm"][..],
        &["--headless", "--config", "no/such/paperpaw.ini"][..],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
            .args(args)
            .current_dir(&cwd)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} ran");
    }
    let written = files_under(&cwd);
    let _ = fs::remove_dir_all(&cwd);
    assert!(written.is_empty(), "wrote {written:?}");
}

/// Runs `script` headless on `profile` in the library `library`, with
/// `args` more.
fn run_in(library: &Path, profile: &str, script: &str, args: &[&str]) -> Run {
    Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(["--profile", profile, "--headless", "--library"])
        .arg(library)
        .args(["--script", script])
        .args(args)
        .output()
        .unwrap()
        .into()
}

/// The same in a fresh library named `name`, no book open at start.
fn run(name: &str, profile: &str, script: &str, args: &[&str]) -> Run {
    let library = library(name);
    let mut all = vec!["--set", "reader.open-last=false"];
    all.extend_from_slice(args);
    let run = run_in(&library, profile, script, &all);
    let _ = fs::remove_dir_all(&library);
    run
}

/// A PNG's width and height.
fn size(path: &Path) -> (u32, u32) {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).unwrap()));
    let reader = decoder.read_info().unwrap();
    let info = reader.info();
    (info.width, info.height)
}

#[test]
fn help_and_the_profiles_are_printed() {
    let help = emulator(&["--help"]);
    assert!(help.ok);
    assert!(help.stdout.contains("USAGE:"), "{}", help.stdout);
    assert!(help.stdout.contains("--heap-budget"));
    let profiles = emulator(&["--list-profiles"]);
    assert!(profiles.ok);
    for id in [
        "xteink-x4",
        "xteink-x4-pro",
        "boox-go-7",
        "kobo-libra-colour",
    ] {
        assert!(
            profiles.stdout.lines().any(|l| l.starts_with(id)),
            "{id}: {}",
            profiles.stdout
        );
    }
}

/// Wrong arguments end the run with a message saying what is wrong,
/// before any window could open.
#[test]
fn wrong_arguments_are_errors_that_say_why() {
    let cases: &[(&[&str], &str)] = &[
        (&["--frobnicate"], "unknown argument `--frobnicate`"),
        (
            &["--headless", "--profile", "nope"],
            "unknown profile `nope`",
        ),
        (&["--headless", "--profile"], "--profile requires a value"),
        (
            &["--headless", "--set", "no-value"],
            "--set expects <key>=<value>",
        ),
        (
            &["--headless", "--set", "nosection=1"],
            "--set expects <section>.<key>=<value>",
        ),
        (
            &["--headless", "--heap-budget", "lots"],
            "--heap-budget expects KiB or `none`",
        ),
        (
            &["--headless", "--trace-alloc", "big"],
            "--trace-alloc expects bytes",
        ),
        (&["--heap-report"], "--heap-report requires --headless"),
        (
            &["--script", "confirm"],
            "--script and --screenshot require --headless",
        ),
        (
            &["--headless", "--config", "no/such/paperpaw.ini"],
            "cannot read no/such/paperpaw.ini",
        ),
    ];
    for (args, says) in cases {
        let run = emulator(args);
        assert!(!run.ok, "{args:?} ran");
        assert!(run.stderr.contains(says), "{args:?}: {}", run.stderr);
    }
}

/// A step the device cannot do, or that is written wrong, stops the
/// script with the step and the reason.
#[test]
fn wrong_script_steps_are_errors_that_name_the_step() {
    let cases = [
        ("xteink-x4", "tap:10:10", "has no touch screen"),
        ("xteink-x4", "swipe:1:1:2:2", "has no touch screen"),
        ("xteink-x4", "menu", "has no button `menu`"),
        ("xteink-x4", "long:home", "has no button `home`"),
        ("xteink-x4", "wait:soon", "`soon` is not a number"),
        ("xteink-x4", "serve:ever", "`ever` is not a number"),
        ("xteink-x4", "a:b:c:d:e:f", "unknown step"),
        ("xteink-x4", "run:Not An Id", "step `run:Not An Id`"),
        ("xteink-x4", "type:hi", "no text field shows"),
        (
            "xteink-x4",
            "open:settings.group:novalue",
            "`novalue` is no key=value",
        ),
        ("boox-go-7", "tap:ten:10", "`ten` is not a number"),
        (
            "boox-go-7",
            "tap-on:Nothing says this",
            "nothing on the screen says",
        ),
    ];
    let library = library("steps");
    for (profile, step, says) in cases {
        let run = run_in(
            &library,
            profile,
            step,
            &["--set", "reader.open-last=false"],
        );
        assert!(!run.ok, "{profile} {step} ran");
        assert!(
            run.stderr.contains(&format!("step `{step}`")),
            "{step}: {}",
            run.stderr
        );
        assert!(run.stderr.contains(says), "{step}: {}", run.stderr);
    }
    let _ = fs::remove_dir_all(&library);
}

/// `shot:` saves the screen in the middle of a script, `--screenshot`
/// at its end; `redraw` draws the same again.
#[test]
fn screenshots_are_taken_during_and_after_a_script() {
    let library = library("shots");
    let during = library.join("out/home.png");
    let after = library.join("menu.png");
    let script = format!("shot:{},redraw,run:core.menu", during.display());
    let run = run_in(
        &library,
        "xteink-x4",
        &script,
        &[
            "--set",
            "reader.open-last=false",
            "--screenshot",
            after.to_str().unwrap(),
        ],
    );
    assert!(run.ok, "{}", run.stderr);
    assert_eq!(run.screen(), "menu.list");
    // The X4 held upright: 480 wide, 800 high.
    assert_eq!(size(&during), (480, 800));
    assert_eq!(size(&after), (480, 800));
    assert_ne!(fs::read(&during).unwrap(), fs::read(&after).unwrap());
    let _ = fs::remove_dir_all(&library);
}

/// The power button puts the device to sleep (the sleep screen shown,
/// plugins told to save); asleep, other buttons do nothing, and power
/// wakes it where it was.
#[test]
fn power_sleeps_and_only_power_wakes() {
    let asleep = run(
        "sleep",
        "xteink-x4",
        "power,wait:500,page-forward,confirm",
        &[],
    );
    assert!(asleep.ok, "{}", asleep.stderr);
    assert!(
        asleep.stderr.contains("xteink-x4: asleep"),
        "{}",
        asleep.stderr
    );
    assert!(!asleep.stderr.contains("awake"));
    assert_eq!(asleep.screen(), "sleep.screen");
    let woken = run(
        "wake",
        "xteink-x4",
        "power,wait:500,page-forward,power,wait:500",
        &[],
    );
    assert!(woken.ok, "{}", woken.stderr);
    assert!(
        woken.stderr.contains("xteink-x4: awake"),
        "{}",
        woken.stderr
    );
    assert_eq!(woken.screen(), "home.dashboard");
}

/// `type:` types into the keyboard shown as if on its keys, umlauts and
/// all; `type-enter:` confirms what was typed, and the keyboard closes.
#[test]
fn text_is_typed_into_the_keyboard_shown() {
    let typed = run(
        "type",
        "xteink-x4",
        "open:keyboard.screen:title=Name,type:Grüße Zoë,describe,type-enter:!",
        &[],
    );
    assert!(typed.ok, "{}", typed.stderr);
    assert!(typed.stdout.contains("Grüße Zoë"), "{}", typed.stdout);
    assert_ne!(typed.screen(), "keyboard.screen", "{}", typed.stdout);
}

/// `run:` reaches an action a native plugin declares, and what the plugin
/// answers (`open` its screen) is done in the call that ran it: the transfer
/// plugin's `transfer.open` opens its screen as `core.menu` opens the menu.
#[test]
fn an_action_a_native_plugin_declares_opens_its_screen() {
    let opened = run(
        "native-action",
        "xteink-x4",
        "run:transfer.open,describe",
        &[],
    );
    assert!(opened.ok, "{}", opened.stderr);
    assert_eq!(opened.screen(), "transfer.screen", "{}", opened.stdout);
}

/// The screen turned, by the configuration at start or chosen in the
/// settings while running: the panel trades width and height.
#[test]
fn turning_the_screen_trades_the_panels_width_and_height() {
    let library = library("turned");
    let shot = library.join("shot.png");
    let shot_arg = shot.to_str().unwrap();
    let upright = run_in(
        &library,
        "boox-go-7",
        "",
        &["--set", "reader.open-last=false", "--screenshot", shot_arg],
    );
    assert!(upright.ok, "{}", upright.stderr);
    assert_eq!(size(&shot), (1264, 1680));
    let configured = run_in(
        &library,
        "boox-go-7",
        "",
        &[
            "--set",
            "reader.open-last=false",
            "--set",
            "core.rotation=core.rotation.landscape",
            "--screenshot",
            shot_arg,
        ],
    );
    assert!(configured.ok, "{}", configured.stderr);
    assert_eq!(size(&shot), (1680, 1264));
    let chosen = run_in(
        &library,
        "boox-go-7",
        "open:settings.choose:id=core.rotation,wait:300,tap-on:On its side,wait:300",
        &["--set", "reader.open-last=false", "--screenshot", shot_arg],
    );
    assert!(chosen.ok, "{}", chosen.stderr);
    assert_eq!(size(&shot), (1680, 1264));
    // Kept: written to the configuration on the card.
    let ini = fs::read_to_string(library.join("paperpaw.ini")).unwrap();
    assert!(ini.contains("rotation = core.rotation.landscape"), "{ini}");
    let _ = fs::remove_dir_all(&library);
}

/// A plugin switched on in the plugin manager applies after the restart
/// it offers: the emulated device builds its kernel anew, the plugin on.
/// (Browser is on the first page of the list; About is native and no longer
/// among the plugins of the app, so the list is shorter.)
#[test]
#[ignore = "BLOCKED (#228): the interface-7 plugin manager waits for the host (H1/H3/H4) and has no restart row until the DEV command senders (#342)"]
fn a_plugin_switched_on_applies_after_the_restart() {
    let run = run(
        "restart",
        "boox-go-7",
        "open:plugin-manager.list,wait:300,\
         tap-on:Browser,wait:300,tap-on:Restart now,wait:500,open:plugin-manager.list,wait:300,describe",
        &[],
    );
    assert!(run.ok, "{}", run.stderr);
    assert!(run.stdout.contains("Row \"Browser, On,"), "{}", run.stdout);
    // Asked for no restart again.
    assert!(!run.stdout.contains("Restart now"), "{}", run.stdout);
}

/// Reading aloud where the device speaks: each passage logged, the next
/// spoken when the one before has ended.
#[test]
fn reading_aloud_speaks_passage_after_passage() {
    let library = library("speech");
    let state = library.join(".paperpaw/plugins/reader");
    fs::create_dir_all(&state).unwrap();
    fs::write(
        state.join("state.ini"),
        "[reader]\nlast = books/alice-in-wonderland.txt\n",
    )
    .unwrap();
    let aloud = run_in(
        &library,
        "boox-go-7",
        "run:reader.read-aloud,wait:15000",
        &[],
    );
    let _ = fs::remove_dir_all(&library);
    assert!(aloud.ok, "{}", aloud.stderr);
    assert_eq!(aloud.screen(), "reader.screen");
    let spoken = aloud
        .stderr
        .lines()
        .filter(|l| l.starts_with("speech (en, 100 %"))
        .count();
    assert!(spoken >= 2, "{}", aloud.stderr);
    // The X4 does not speak: nothing is said there.
    let silent = run(
        "silent",
        "xteink-x4",
        "run:reader.read-aloud,wait:3000",
        &[],
    );
    assert!(!silent.stderr.contains("speech ("), "{}", silent.stderr);
}

/// A configuration file (`--config`) is read, and `--set` wins over it.
#[test]
fn the_configuration_file_is_read_and_set_wins_over_it() {
    let library = library("config");
    let config = library.join("user.ini");
    fs::write(&config, "[core]\nlanguage = de\nnight-mode = on\n").unwrap();
    let config_arg = config.to_str().unwrap();
    let german = run_in(
        &library,
        "boox-go-7",
        "run:core.menu,describe",
        &["--set", "reader.open-last=false", "--config", config_arg],
    );
    assert!(german.ok, "{}", german.stderr);
    assert!(
        german.stdout.contains("Row \"Einstellungen\""),
        "{}",
        german.stdout
    );
    assert!(
        german.stdout.contains("(on)"),
        "night mode on: {}",
        german.stdout
    );
    let english = run_in(
        &library,
        "boox-go-7",
        "run:core.menu,describe",
        &[
            "--set",
            "reader.open-last=false",
            "--config",
            config_arg,
            "--set",
            "core.language=en",
        ],
    );
    assert!(english.ok, "{}", english.stderr);
    assert!(
        english.stdout.contains("Row \"Settings\""),
        "{}",
        english.stdout
    );
    let _ = fs::remove_dir_all(&library);
}

/// `--wifi online` (the workshop's): the reader starts in a network, stays
/// in it when nothing uses it, and Wi-Fi switches off and on in the
/// settings as on a device. Without it the radio starts off.
#[test]
#[ignore = "needs native activation (Wi-Fi is a native-only plugin since #244 and the desktop does not offer uses.wifi: the Wi-Fi page is not there)"]
fn online_from_the_start_with_wifi_online() {
    let connected = "Wi-Fi, Connected to Workshop Wi-Fi";
    let online = run(
        "wifi-online",
        "xteink-x4",
        "run:wifi.open,describe,run:core.home,wait:120000,run:wifi.open,describe,\
         confirm,wait:3000,describe,confirm,wait:6000,describe",
        &["--wifi", "online"],
    );
    assert!(online.ok, "{}", online.stderr);
    let states: Vec<&str> = online
        .stdout
        .lines()
        .filter(|line| line.starts_with("Row \"Wi-Fi, "))
        .collect();
    assert_eq!(states.len(), 4, "{}", online.stdout);
    assert!(states[0].contains(connected), "at once: {}", states[0]);
    assert!(
        states[1].contains(connected),
        "two minutes on: {}",
        states[1]
    );
    assert!(
        states[2].contains("Wi-Fi, Off"),
        "switched off: {}",
        states[2]
    );
    assert!(states[3].contains(connected), "switched on: {}", states[3]);

    let off = run("wifi-off", "xteink-x4", "run:wifi.open,describe", &[]);
    assert!(off.ok, "{}", off.stderr);
    assert!(off.stdout.contains("Row \"Wi-Fi, Off\""), "{}", off.stdout);
}

/// `--wifi online` before `--stdio`, as the workshop passes it: the
/// devices it starts are online; the help names it so the workshop can
/// tell.
#[test]
#[ignore = "needs native activation (Wi-Fi is a native-only plugin since #244 and the desktop does not offer uses.wifi: the Wi-Fi page is not there)"]
fn wifi_online_holds_for_the_devices_stdio_starts() {
    use std::io::Write;
    let help = emulator(&["--help"]);
    assert!(
        help.stdout
            .lines()
            .any(|l| l.trim_start().starts_with("--wifi <state>")),
        "{}",
        help.stdout
    );
    let library = library("wifi-stdio");
    let path = paperpaw_core::sdk::json::string(&library.display().to_string());
    let mut child = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"))
        .args(["--wifi", "online", "--stdio"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let lines = format!(
        "{{\"op\": \"start\", \"profile\": \"xteink-x4\", \"library\": {path}}}\n\
         {{\"op\": \"open\", \"screen\": \"wifi.page\"}}\n\
         {{\"op\": \"describe\"}}\n\
         {{\"op\": \"quit\"}}\n"
    );
    child
        .stdin
        .take()
        .unwrap()
        .write_all(lines.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let _ = fs::remove_dir_all(&library);
    assert!(
        stdout.contains("Connected to Workshop Wi-Fi"),
        "{}",
        stdout
            .lines()
            .filter(|l| !l.contains("\"frame\""))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// The height of the first row of the settings list and how many rows it
/// shows on the first screen, on the X4 with the interface text set to
/// `size` points (the default, 8, where `None`).
fn settings_rows(size: Option<u32>) -> (u32, usize) {
    let set = size.map(|n| format!("core.ui-size={n}"));
    let args: Vec<&str> = set.iter().flat_map(|s| ["--set", s.as_str()]).collect();
    let run = run(
        "rows",
        "xteink-x4",
        "run:core.settings,wait:300,describe",
        &args,
    );
    assert!(run.ok, "{}", run.stderr);
    // `Row "…" at x,y WxH`
    let heights: Vec<u32> = run
        .stdout
        .lines()
        .filter(|l| l.starts_with("Row "))
        .filter_map(|l| l.rsplit_once(" at ")?.1.rsplit_once('x')?.1.parse().ok())
        .collect();
    (heights[0], heights.len())
}

/// Smaller interface text never makes rows taller: the fonts come in steps
/// and the least line of one is as high as a size above it, which once
/// made the rows of 8 pt (73 px on the X4) taller than those of 9 pt (63),
/// and the list show fewer rows with smaller text (#119).
#[test]
fn rows_do_not_grow_as_the_interface_text_gets_smaller() {
    let rows: Vec<(u32, (u32, usize))> = (7..=24).map(|n| (n, settings_rows(Some(n)))).collect();
    for pair in rows.windows(2) {
        let ((small, (low, _)), (large, (high, _))) = (pair[0], pair[1]);
        assert!(
            low <= high,
            "rows of {small} pt ({low} px) are taller than of {large} pt ({high} px): {rows:?}"
        );
    }
    // The default is 8 pt, and it shows at least as many rows as 9 pt.
    let default = settings_rows(None);
    assert_eq!(default, rows[1].1, "the default is 8 pt: {rows:?}");
    assert!(default.1 >= rows[2].1.1, "{rows:?}");
    // From 10 pt up the rows are as they were: 24 pt (whose font steps)
    // keeps its 153 px, not the 118 px of the two least lines.
    assert_eq!(rows[17].1.0, 153, "{rows:?}");
    eprintln!("settings rows (height, count) by size: {rows:?}");
}
