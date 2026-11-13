//! Memory on the small devices: the emulator charges what the kernel
//! and its plugins allocate against the profile's heap budget (256 KiB
//! on the X4, see `src/budget.rs`) and refuses what goes past it, as the
//! device would. Here the main flows run headless on the X4 profiles: a
//! book opened (EPUB and TXT), 30 pages forward and 30 back, home, the
//! library (with many books) opened and scrolled, the main menu, the
//! settings and a page of them; a long Word document and a hostile one,
//! converted on opening; and the store, online with the
//! emulator's own catalogue: its list, a plugin's page and the install of
//! a plugin (the TLS session and the Wi-Fi stack are not modelled: a
//! measured allowance stands for them).
//!
//! Each flow runs twice, under the profile's budget and without one. The
//! runs are deterministic (simulated time, a fixed clock), so the same
//! peak in both says no allocation was refused under the budget; the peak
//! must also stay a margin below the budget, room for what the emulator
//! does not model (fragmentation, the stack) and for growth to be noticed
//! before it fails on a device.
//!
//! A budget too small stops a run as the device would, and the heap
//! report lists what is live by call site.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// The share of the budget, in percent, the peak may reach at most.
const MARGIN_PERCENT: f64 = 85.0;
/// What a connected remote may add to the peak of reading, at most.
const BLUETOOTH_KIB: f64 = 8.0;
/// What the device's store run held beyond what the emulator charges for
/// the same run, in KiB: the TLS session (mbedTLS' buffers and the
/// server's chain), the Wi-Fi stack's buffers and what is not modelled.
/// The emulator answers the store from files, so none of it shows here.
/// Measured on the X4 on 24 October (ticket #8): 184.4 KiB with the store
/// open, against 134.5 KiB in the emulator's run of the same screens
/// (the store test below). To be set again from a new measurement on
/// the device.
const STORE_DEVICE_EXTRA_KIB: f64 = 50.0;

/// The repository: two folders above this crate.
fn root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .ancestors()
        .nth(2)
        .unwrap_or(manifest)
        .to_path_buf()
}

/// A fresh library named `name`: the sample books, sixty small ones more
/// so that the library has pages to scroll, and `book` open at start.
fn library(name: &str, book: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("paperpaw-memory-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    let books = dir.join("books");
    fs::create_dir_all(&books).unwrap();
    for entry in fs::read_dir(root().join("samples/library/books")).unwrap() {
        let entry = entry.unwrap();
        fs::copy(entry.path(), books.join(entry.file_name())).unwrap();
    }
    for n in 0..60 {
        fs::write(
            books.join(format!("book-{n:02}.txt")),
            format!("Book {n}\n\nA short book, number {n}, to fill the library.\n"),
        )
        .unwrap();
    }
    let setup = dir.join(".paperpaw/plugins/setup");
    fs::create_dir_all(&setup).unwrap();
    fs::write(setup.join("done"), "").unwrap();
    if let Some(book) = book {
        let state = dir.join(".paperpaw/plugins/reader");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("state.ini"),
            format!("[reader]\nlast = {book}\n"),
        )
        .unwrap();
    }
    dir
}

/// Runs the emulator headless on `profile` in a fresh library.
fn emulator(name: &str, profile: &str, book: Option<&str>, args: &[&str]) -> Output {
    emulator_with(name, profile, book, &[], args)
}

/// [`emulator`] with `files` (path in the library, content) written first.
fn emulator_with(
    name: &str,
    profile: &str,
    book: Option<&str>,
    files: &[(&str, Vec<u8>)],
    args: &[&str],
) -> Output {
    let library = library(name, book);
    for (path, content) in files {
        let path = library.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }
    let mut command = run_in(&library, profile, args);
    if book.is_none() {
        command.args(["--set", "reader.open-last=false"]);
    }
    let output = command.output().unwrap();
    let _ = fs::remove_dir_all(&library);
    output
}

/// The emulator headless on `profile` in `library`, with `args`.
fn run_in(library: &Path, profile: &str, args: &[&str]) -> Command {
    // No core dump: a budget too small ends the run with `abort()` (the
    // allocation error handler), and where the shell allows core files that
    // leaves a megabyte-sized `core.<pid>` in the crate's folder.
    #[cfg(unix)]
    let mut command = {
        let mut sh = Command::new("sh");
        sh.args(["-c", "ulimit -c 0; exec \"$0\" \"$@\""])
            .arg(env!("CARGO_BIN_EXE_paperpaw-emu"));
        sh
    };
    #[cfg(not(unix))]
    let mut command = Command::new(env!("CARGO_BIN_EXE_paperpaw-emu"));
    command
        .args(["--profile", profile, "--headless", "--library"])
        .arg(library)
        .args(["--set", "help.show=false"])
        .args(args);
    command
}

/// What the last line of a run says: the screen shown, the heap's peak
/// and the budget (KiB), e.g.
/// `xteink-x4: screen home.dashboard, heap 119.3 KiB / 256 KiB (peak 188.3 KiB)`.
#[derive(Debug, PartialEq)]
struct Heap {
    screen: String,
    peak: f64,
    limit: Option<f64>,
}

fn heap(output: &Output) -> Heap {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout.lines().last().expect("a summary line");
    let number = |after: &str| -> Option<f64> {
        let rest = &line[line.find(after)? + after.len()..];
        rest.split_whitespace().next()?.parse().ok()
    };
    let screen = line
        .split("screen ")
        .nth(1)
        .and_then(|s| s.split(',').next())
        .unwrap_or_default()
        .to_string();
    Heap {
        screen,
        peak: number("(peak ").expect(line),
        limit: number(" / "),
    }
}

/// `count` presses of the button `step`.
fn times(step: &str, count: usize) -> String {
    vec![step; count].join(",")
}

/// Reads 30 pages forward and 30 back, goes home, opens the library and
/// scrolls it, the main menu, the settings and the first page of them.
/// Actions run as buttons bound to them would, so the same flow works on
/// both X4 profiles (the X4 Pro has no *back* button).
fn reading_flow() -> String {
    [
        times("page-forward", 30),
        times("page-back", 30),
        "run:core.home,wait:300,run:core.library,wait:300".into(),
        times("page-forward", 3),
        "page-back,run:core.back,run:core.menu,wait:300,run:core.back".into(),
        "run:core.settings,wait:300,run:core.down,run:core.confirm,wait:300".into(),
        "run:core.back,run:core.back,run:core.home".into(),
    ]
    .join(",")
}

/// Runs the reading flow with `book` on `profile` under its budget and
/// without one, and checks the peak.
fn stays_within_budget(name: &str, profile: &str, book: &str) -> Heap {
    flow_stays_within_budget(name, profile, book, &reading_flow(), &[], &[])
}

/// Runs `flow` with `book` on `profile` (with `files` in the library and
/// `args`) under its budget and without one, and checks the peak.
fn flow_stays_within_budget(
    name: &str,
    profile: &str,
    book: &str,
    flow: &str,
    files: &[(&str, Vec<u8>)],
    args: &[&str],
) -> Heap {
    let with = |extra: &[&str]| {
        let mut all: Vec<&str> = args.to_vec();
        all.extend_from_slice(extra);
        all.extend_from_slice(&["--script", flow]);
        heap(&emulator_with(name, profile, Some(book), files, &all))
    };
    let budgeted = with(&[]);
    let free = with(&["--heap-budget", "none"]);
    let limit = budgeted.limit.expect("the profile has a budget");
    assert_eq!(free.limit, None);
    assert_eq!(budgeted.screen, "home.dashboard", "the flow went home");
    assert_eq!(free.screen, budgeted.screen);
    // The same allocations with and without the budget: none refused.
    assert!(
        (budgeted.peak - free.peak).abs() < 0.05,
        "{name}: peak {} KiB under the budget, {} KiB without",
        budgeted.peak,
        free.peak
    );
    assert!(
        free.peak <= limit * MARGIN_PERCENT / 100.0,
        "{name}: peak {} KiB of {limit} KiB, over {MARGIN_PERCENT} %",
        free.peak
    );
    budgeted
}

/// How far German may start above English's peak: its own peak is
/// reading in its longer catalogs. Both started at 135.9 KiB while the
/// plugin list doubled and a start's key map was made beside the old one;
/// since then (#8) English peaks at 123.3 KiB, German at 133.4: what
/// keeps German in check is [`GERMAN_START_CEILING_KIB`]. On the
/// integration state of 2026-11-01 German peaks at 136.0 KiB, as it did
/// before (136.2 KiB at f8a0aa4a and 4b14de2e, where English peaked at the
/// same 136.2), while English fell to 123.4 KiB with kernel/store-memory
/// (68e03ab9): the German catalogs are read in after the point where
/// English peaks, so German no longer rides on English's peak. Measured
/// at those three states; the German start did not grow.
const GERMAN_START_EXTRA_KIB: f64 = 13.0;
/// And at most this, whatever English does: the 136.0 KiB German starts
/// with, 137.7 KiB with format-images (it alone added 1.0 KiB to the
/// German start, 3.8 KiB to the English one), and a little room. Keep it below 140 KiB, where
/// the store's install peaks sit with German on top (#219).
const GERMAN_START_CEILING_KIB: f64 = 139.0;

/// The language catalogs are read in once the registries have given back
/// what they reserved, not before: German (longer texts than English)
/// starts within [`GERMAN_START_EXTRA_KIB`] of the peak of English, and
/// under [`GERMAN_START_CEILING_KIB`].
#[test]
fn starting_in_german_peaks_no_higher_than_in_english() {
    let peak = |language: &str| {
        let language = format!("core.language={language}");
        heap(&emulator(
            "x4-start",
            "xteink-x4",
            None,
            &["--set", &language, "--script", "wait:300"],
        ))
        .peak
    };
    let (english, german) = (peak("en"), peak("de"));
    eprintln!("start: peak {english} KiB in English, {german} KiB in German");
    assert!(
        german <= english + GERMAN_START_EXTRA_KIB,
        "start: peak {german} KiB in German, {english} KiB in English"
    );
    assert!(
        german <= GERMAN_START_CEILING_KIB,
        "start: peak {german} KiB in German, over {GERMAN_START_CEILING_KIB} KiB"
    );
}

/// What a language of all plugins' texts from the card may add to the
/// start's peak, compiled and read as needed (ADR 0033): its block table
/// and the messages of the first screen, not its 210 KB.
///
/// 10.0 KiB since the start settles its first passes before its first frame
/// (#243): native plugins hear `Started` while the first frame is built, and
/// read their card-language texts then, a transient overlap. Measured on the
/// emulator X4 profile: English / Arabic peak 111.4 / 119.0 KiB before,
/// 111.8 / 121.7 KiB after (121.7 of 256 KiB). A later saving is noted on #248.
const CARD_LANGUAGE_EXTRA_KIB: f64 = 10.0;

/// The files of a card language of an Arabic interface's size (3,400 messages,
/// some 210 KB): its plugin folder and the compiled catalog.
fn arabic_card_files() -> [(&'static str, Vec<u8>); 2] {
    let mut source = String::from(
        "[meta]\nlanguage = ar\nname = العربية\nplural = arabic\ndirection = rtl\n[messages]\n",
    );
    for n in 0..3400 {
        source.push_str(&format!(
            "filler.{n:04} = نص الرسالة رقم {n} لواجهة المستخدم في الجهاز\n"
        ));
    }
    assert!(source.len() > 200_000, "{}", source.len());
    let compiled = paperpaw_i18n::Catalog::compile(&source).unwrap();
    [
        (
            "plugins/arabic/plugin.ini",
            b"[plugin]\nname = Arabic\nformats = 4\n".to_vec(),
        ),
        ("plugins/arabic/i18n/ar.ppcat", compiled),
    ]
}

/// The start with the card language and the book read last an EPUB (the
/// largest first screen): the peak stays within the X4's budget (#243).
#[test]
fn starting_on_an_epub_in_a_large_card_language_stays_within_the_x4_budget() {
    let files = arabic_card_files();
    let output = emulator_with(
        "x4-card-language-epub",
        "xteink-x4",
        Some("books/alice-in-wonderland.epub"),
        &files,
        &["--set", "core.language=ar", "--script", "wait:300"],
    );
    let heap = heap(&output);
    let limit = heap.limit.expect("the profile has a budget");
    eprintln!(
        "start: peak {} KiB in a 210 KB language from the card, an EPUB read last (screen {})",
        heap.peak, heap.screen
    );
    assert_eq!(heap.screen, "reader.screen");
    assert!(
        heap.peak <= limit * MARGIN_PERCENT / 100.0,
        "peak {} KiB of {limit} KiB, over {MARGIN_PERCENT} %",
        heap.peak
    );
}

/// A language from the card, of an Arabic interface's size (3,400
/// messages, some 210 KB), starts within [`CARD_LANGUAGE_EXTRA_KIB`] of
/// English's peak.
#[test]
fn starting_in_a_large_language_from_the_card_peaks_little_above_english() {
    let files = arabic_card_files();
    let peak = |language: &str| {
        let language = format!("core.language={language}");
        heap(&emulator_with(
            "x4-card-language",
            "xteink-x4",
            None,
            &files,
            &["--set", &language, "--script", "wait:300"],
        ))
        .peak
    };
    let (english, arabic) = (peak("en"), peak("ar"));
    eprintln!(
        "start: peak {english} KiB in English, {arabic} KiB in a 210 KB language from the card"
    );
    assert!(
        arabic <= english + CARD_LANGUAGE_EXTRA_KIB,
        "start: peak {arabic} KiB in a language from the card, {english} KiB in English"
    );
}

#[test]
fn reading_an_epub_stays_within_the_x4_budget() {
    let heap = stays_within_budget("x4-epub", "xteink-x4", "books/alice-in-wonderland.epub");
    assert_eq!(heap.limit, Some(256.0));
}

#[test]
fn reading_a_txt_stays_within_the_x4_budget() {
    stays_within_budget("x4-txt", "xteink-x4", "books/die-verwandlung.txt");
}

/// The store on the X4, online with the emulator's own catalogue: its
/// list, a group and a plugin's page, then home.
#[test]
fn the_store_stays_within_the_x4_budget() {
    let flow = [
        "run:store.open,wait:3000",
        "run:core.down,run:core.down,run:core.confirm,wait:2000",
        "run:core.down,run:core.confirm,wait:3000",
        "run:core.back,run:core.back,run:core.back,run:core.home,wait:300",
    ]
    .join(",");
    flow_stays_within_budget(
        "x4-store",
        "xteink-x4",
        "books/die-verwandlung.txt",
        &flow,
        &[],
        &["--network", "fixtures", "--wifi", "online"],
    );
}

/// Opens the store's list, the group *Reading* and a plugin's page of it,
/// presses *Install*, looks at the page's screenshots and goes home. The
/// plugin is a data plugin of the emulator's catalogue: the download of
/// its files, their check and their install run as on a device (the
/// shell's installer and the plugin folder on the card).
fn installing_flow() -> String {
    [
        "run:store.open,wait:3000",
        "run:core.down,run:core.down,run:core.confirm,wait:2000",
        "run:core.down,run:core.down,run:core.confirm,wait:3000",
        "run:core.confirm,wait:5000",
        "run:core.down,run:core.confirm,wait:3000",
        "run:core.back,run:core.back,run:core.back,run:core.back,run:core.home,wait:300",
    ]
    .join(",")
}

/// A plugin installed from the store on the X4: its page with the
/// screenshots, the download and the install, then the screenshots page.
#[test]
fn installing_from_the_store_stays_within_the_x4_budget() {
    let heap = flow_stays_within_budget(
        "x4-store-install",
        "xteink-x4",
        "books/die-verwandlung.txt",
        &installing_flow(),
        &[],
        &["--network", "fixtures", "--wifi", "online"],
    );
    eprintln!("x4 store install: peak {} KiB", heap.peak);
}

/// The store on the X4 with what the emulator cannot charge: the run's
/// peak plus [`STORE_DEVICE_EXTRA_KIB`] (the TLS session and the Wi-Fi
/// stack of the device) stays under the margin of the budget, so that
/// the store's growth is noticed before the device runs out. The
/// emulator's part is the whole run's peak, and the peak from the flow's
/// start (reset there, the book still open beneath).
#[test]
fn the_store_with_the_devices_tls_stays_within_the_x4_budget() {
    let flow = format!("wait:300,reset-peak,{}", installing_flow());
    let run = |name: &str, flow: &str| {
        let library = library(name, Some("books/die-verwandlung.txt"));
        let mut command = run_in(&library, "xteink-x4", &[]);
        command
            .args(["--network", "fixtures", "--wifi", "online"])
            .args(["--heap-budget", "none", "--script", flow]);
        let output = command.output().unwrap();
        // The flow installed the plugin: without it the peaks say little.
        let installed = library.join("plugins/reading-aids/plugin.ini").exists();
        let _ = fs::remove_dir_all(&library);
        assert!(installed, "{name}: the plugin was not installed");
        heap(&output).peak
    };
    let store = run("x4-store-peak", &flow);
    let whole = run("x4-store-whole", &installing_flow());
    let allowed = 256.0 * MARGIN_PERCENT / 100.0;
    eprintln!(
        "x4 store: peak {store} KiB from the store's start, {whole} KiB in the run; + {STORE_DEVICE_EXTRA_KIB} KiB of the device = {} KiB of {allowed} KiB",
        whole + STORE_DEVICE_EXTRA_KIB
    );
    assert!(
        whole + STORE_DEVICE_EXTRA_KIB <= allowed,
        "store: peak {whole} KiB + {STORE_DEVICE_EXTRA_KIB} KiB of the device's TLS and Wi-Fi, over {allowed} KiB"
    );
    assert!(store <= whole);
}

/// What installing a plugin from the store may add on the X4 to the heap
/// before it, at most (#8). The first install of a session takes the room
/// for one more plugin the start left (6.2 KiB in all; 19.2 when the list
/// doubled), a second one grows the list by an eighth, old and new at
/// once (11.6 KiB; 18.8 when it doubled). The key map is made anew without
/// the old one beside it.
const INSTALL_GROWTH_KIB: f64 = 13.0;

/// The store on the X4 step by step (`step:` in the flow): its list, a
/// group, a plugin's page, its install (download, check, install), its
/// screenshots, and a second plugin's page and install in the same
/// session, each its peak from the end of the step before. Each with
/// what the device holds for the network beside it
/// ([`STORE_DEVICE_EXTRA_KIB`]: the TLS handshake and session, the
/// sockets and the Wi-Fi, which the emulator's own catalogue does not
/// take) stays under the margin of the budget, and each install grows the
/// heap by [`INSTALL_GROWTH_KIB`] at most.
#[test]
fn each_step_of_the_store_stays_within_the_x4_budget() {
    let steps = [
        "list", "group", "page", "install", "shots", "page2", "install2",
    ];
    let parts = [
        "run:store.open,wait:3000",
        "run:core.down,run:core.down,run:core.confirm,wait:2000",
        "run:core.down,run:core.down,run:core.confirm,wait:3000",
        "run:core.confirm,wait:5000",
        "run:core.down,run:core.confirm,wait:3000",
        // Home (not one `back`: the store's page stays on the stack under
        // the native store, `back` goes to it, as the old store went home),
        // the store again (now with a row for what is installed), the group
        // *New* and its first plugin's page (`long-notes`).
        "run:core.home,wait:500,run:store.open,wait:3000,\
         run:core.down,run:core.down,run:core.confirm,wait:2000,\
         run:core.confirm,wait:3000",
        "run:core.confirm,wait:5000",
    ];
    let mut flow = String::from("wait:300,step:start");
    for (step, part) in steps.iter().zip(parts) {
        flow.push_str(&format!(",{part},step:{step}"));
    }
    let library = library("x4-store-steps", Some("books/die-verwandlung.txt"));
    let mut command = run_in(&library, "xteink-x4", &[]);
    command
        .args(["--network", "fixtures", "--wifi", "online"])
        .args(["--heap-budget", "none", "--script", &flow]);
    let output = command.output().unwrap();
    let installed = ["reading-aids", "long-notes"]
        .iter()
        .all(|id| library.join(format!("plugins/{id}/plugin.ini")).exists());
    let _ = fs::remove_dir_all(&library);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(installed, "the plugins were not installed");
    // `step <name>: peak <KiB> KiB, live <KiB> KiB`
    let measured: Vec<(String, f64, f64)> = stdout
        .lines()
        .filter_map(|line| {
            let (name, rest) = line.strip_prefix("step ")?.split_once(": peak ")?;
            let (peak, rest) = rest.split_once(" KiB, live ")?;
            let live = rest.strip_suffix(" KiB")?;
            Some((name.to_string(), peak.parse().ok()?, live.parse().ok()?))
        })
        .collect();
    let names: Vec<&str> = measured.iter().map(|(name, ..)| name.as_str()).collect();
    assert_eq!(names[1..], steps, "{stdout}");
    let allowed = 256.0 * MARGIN_PERCENT / 100.0;
    for pair in measured.windows(2) {
        let ((_, _, before), (name, peak, _)) = (&pair[0], &pair[1]);
        eprintln!(
            "x4 store {name}: peak {peak} KiB (+{:.1} KiB), + {STORE_DEVICE_EXTRA_KIB} KiB of the device of {allowed} KiB",
            peak - before
        );
        assert!(
            peak + STORE_DEVICE_EXTRA_KIB <= allowed,
            "store {name}: peak {peak} KiB + {STORE_DEVICE_EXTRA_KIB} KiB of the device, over {allowed} KiB"
        );
        if name.starts_with("install") {
            assert!(
                peak - before <= INSTALL_GROWTH_KIB,
                "installing grew the heap by {:.1} KiB, more than {INSTALL_GROWTH_KIB} KiB",
                peak - before
            );
        }
    }
}

/// The X4 Pro: its own panel (four grays, other bands), touch beside
/// the buttons.
#[test]
fn reading_an_epub_stays_within_the_x4_pro_budget() {
    stays_within_budget("x4-pro-epub", "xteink-x4-pro", "books/die-verwandlung.epub");
}

/// Bluetooth on the X4 Pro: the emulator's remote bonded before and
/// connected while a book is open, turning pages with its keys, then
/// the Bluetooth page and the rest of the reading flow. The kernel's
/// radio state (the link, the remote's report decoding) fits beside
/// reading.
#[test]
#[ignore = "re-enable at Bluetooth activation: the interface-6 Bluetooth plugin is no longer in the app (its page and `bluetooth.remotes` setting are gone until the native plugin is wired)"]
fn bluetooth_stays_within_the_x4_pro_budget() {
    let mut secrets = paperpaw_core::sdk::secrets::Secrets::default();
    secrets.apply(paperpaw_core::sdk::secrets::SecretChange::keep(
        "core.bluetooth.1",
        "PaperPaw Remote",
        "fa:ke:00:00:00:01	PaperPaw Remote	remote	0		keys",
    ));
    let secrets = (
        paperpaw_core::sdk::secrets::FILE,
        secrets.encode().into_bytes(),
    );
    let flow = [
        "wait:1000".into(),
        times("ble-key:right", 20),
        times("ble-key:left", 20),
        "run:bluetooth.open,wait:300,run:core.back".into(),
        reading_flow(),
    ]
    .join(",");
    let book = "books/die-verwandlung.epub";
    let with = flow_stays_within_budget(
        "x4-pro-bluetooth",
        "xteink-x4-pro",
        book,
        &flow,
        &[secrets],
        &["--set", "bluetooth.remotes=bluetooth.remotes.reading"],
    );
    let without = stays_within_budget("x4-pro-no-bluetooth", "xteink-x4-pro", book);
    eprintln!(
        "x4-pro: peak {} KiB with a remote, {} KiB without",
        with.peak, without.peak
    );
    assert!(
        with.peak <= without.peak + BLUETOOTH_KIB,
        "Bluetooth: peak {} KiB, {} KiB without, over {BLUETOOTH_KIB} KiB more",
        with.peak,
        without.peak
    );
}

/// An EPUB whose package document is large (400 chapters): inflating it
/// takes a window of 32 KiB beside the decoder's 10 KiB.
fn large_package() -> Vec<u8> {
    let (mut items, mut spine) = (String::new(), String::new());
    let mut zip = paperpaw_zip::testing::ZipBuilder::new()
        .stored("mimetype", "application/epub+zip")
        .deflated(
            "META-INF/container.xml",
            "<?xml version=\"1.0\"?><container version=\"1.0\" \
             xmlns=\"urn:oasis:names:tc:opendocument:xmlns:container\"><rootfiles>\
             <rootfile full-path=\"content.opf\" media-type=\"application/oebps-package+xml\"/>\
             </rootfiles></container>",
        );
    for n in 0..400 {
        items.push_str(&format!(
            "<item id=\"c{n}\" href=\"c{n}.xhtml\" media-type=\"application/xhtml+xml\"/>"
        ));
        spine.push_str(&format!("<itemref idref=\"c{n}\"/>"));
        zip = zip.deflated(
            &format!("c{n}.xhtml"),
            format!("<html><body><p>Chapter {n}.</p></body></html>"),
        );
    }
    let opf = format!(
        "<?xml version=\"1.0\"?><package xmlns=\"http://www.idpf.org/2007/opf\" \
         version=\"3.0\" unique-identifier=\"id\"><metadata \
         xmlns:dc=\"http://purl.org/dc/elements/1.1/\"><dc:identifier id=\"id\">large</dc:identifier>\
         <dc:title>Large</dc:title><dc:language>en</dc:language></metadata>\
         <manifest>{items}</manifest><spine>{spine}</spine></package>"
    );
    assert!(opf.len() > 32 * 1024);
    zip.deflated("content.opf", opf).build()
}

/// A book the heap has no room to open, as on an X4 whose memory the
/// Wi-Fi radio holds: *Continue reading* on home opens the reader, which
/// says so in a few words, and the run goes on. On the device, running
/// out on the way stopped it and it started again.
#[test]
fn a_book_without_room_to_open_says_so() {
    let home = ["--set", "reader.open-last=false"];
    let open = "confirm,confirm,wait:500,describe";
    // Under a budget a little over what starting with the sample books
    // takes, home shows; the book needs more than is left. Without one,
    // it opens.
    let started = heap(&emulator("no-room-start", "xteink-x4", None, &[]));
    let budget = format!("{}", started.peak.ceil() as u32 + 4);
    let tight = with_large_book(
        "no-room",
        &[&home[..], &["--heap-budget", &budget, "--script", open]].concat(),
    );
    let free = with_large_book(
        "room",
        &[&home[..], &["--heap-budget", "none", "--script", open]].concat(),
    );
    let catalog = fs::read_to_string(root().join("plugins/reader/i18n/en.ini")).unwrap();
    let notice = catalog
        .lines()
        .find_map(|line| line.strip_prefix("reader.no-memory = "))
        .expect("the notice in the catalog");
    let stdout = String::from_utf8_lossy(&tight.stdout);
    assert_eq!(heap(&tight).screen, "reader.screen", "{stdout}");
    // The notice may be set over several lines: their texts, joined.
    assert!(shown(&stdout).contains(notice), "{stdout}");
    let opened = heap(&free);
    assert_eq!(opened.screen, "reader.screen");
    assert!(opened.peak > budget.parse().unwrap(), "{opened:?}");
    assert!(!shown(&String::from_utf8_lossy(&free.stdout)).contains(notice));
}

/// The texts the screen's description lists (`Text "..." at ...`), joined
/// with spaces: a message set over several lines reads as one.
fn shown(stdout: &str) -> String {
    stdout
        .lines()
        .filter_map(|line| line.strip_prefix("Text \""))
        .filter_map(|line| line.split('"').next())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Runs the emulator on the X4 in a fresh library with the book of
/// [`large_package`] as the one read last, the card on home naming it.
fn with_large_book(name: &str, args: &[&str]) -> Output {
    let book = "books/large.epub";
    let library = library(name, Some(book));
    fs::write(library.join(book), large_package()).unwrap();
    let opened = library.join(".paperpaw/library");
    fs::create_dir_all(&opened).unwrap();
    fs::write(opened.join("opened.txt"), format!("{book}\n")).unwrap();
    let output = run_in(&library, "xteink-x4", args).output().unwrap();
    let _ = fs::remove_dir_all(&library);
    output
}

/// A budget below what the app needs to start: the allocation that does
/// not fit is refused and the run ends with an error, as the device
/// would stop, never quietly over the budget.
#[test]
fn a_budget_too_small_stops_the_run() {
    let output = emulator("too-small", "xteink-x4", None, &["--heap-budget", "64"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("memory allocation of"), "{stderr}");
}

/// A budget from the command line wins over the profile's, and `none`
/// lifts it: the large screens' profiles have none to begin with.
#[test]
fn a_budget_given_wins_over_the_profiles() {
    let given = heap(&emulator(
        "given",
        "xteink-x4",
        None,
        &["--heap-budget", "300"],
    ));
    assert_eq!(given.limit, Some(300.0));
    let boox = heap(&emulator("boox", "boox-go-7", None, &[]));
    assert_eq!(boox.limit, None);
    let capped = heap(&emulator(
        "boox-capped",
        "boox-go-7",
        None,
        &["--heap-budget", "4096"],
    ));
    assert_eq!(capped.limit, Some(4096.0));
    assert!(capped.peak <= 4096.0);
}

/// The heap report after a run: the live heap by call site, largest
/// first, adding up to what the summary line says is live.
#[test]
fn the_heap_report_lists_the_live_heap_by_call_site() {
    let output = emulator("report", "xteink-x4", None, &["--heap-report"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    let summary = stdout
        .lines()
        .find(|l| l.starts_with("xteink-x4: screen"))
        .expect("the summary line");
    let live: f64 = summary
        .split("heap ")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .expect(summary);
    let header = stdout
        .lines()
        .find(|l| l.starts_with("live heap by call site ("))
        .expect("the report's heading");
    let total: f64 = header
        .trim_start_matches("live heap by call site (")
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .expect(header);
    assert!(
        (total - live).abs() < 0.2,
        "report {total} KiB, live {live} KiB"
    );
    // The sites of each part: largest first, in PaperPaw's code.
    let sizes = |heading: &str| -> Vec<f64> {
        stdout
            .lines()
            .skip_while(|l| !l.starts_with(heading))
            .skip(1)
            .take_while(|l| l.starts_with(' '))
            .filter(|l| l.contains(" KiB "))
            .map(|l| l.split_whitespace().next().unwrap().parse().unwrap())
            .collect()
    };
    for heading in ["live heap by call site", "heap at the peak by call site"] {
        let sizes = sizes(heading);
        assert!(sizes.len() > 3, "{heading}: {stdout}");
        assert!(
            sizes.windows(2).all(|w| w[0] >= w[1]),
            "{heading}: {sizes:?}"
        );
    }
    assert!(stdout.contains("paperpaw_"), "{stdout}");
    // What was live at the peak: no less than what is live at the end.
    let at_peak: f64 = stdout
        .lines()
        .find_map(|l| l.strip_prefix("heap at the peak by call site ("))
        .and_then(|s| s.split_whitespace().next())
        .and_then(|s| s.parse().ok())
        .expect("the peak's heading");
    assert!(
        at_peak >= live - 0.2,
        "at the peak {at_peak} KiB, live {live} KiB"
    );
}

const WORD_NS: &str = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\" \
    xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" \
    xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" \
    xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"";

/// A Word document with the body `body` and the parts it needs: the picture
/// `rId1`, a link `rId2`, a heading style and two lists.
fn word_document(body: &str) -> Vec<u8> {
    let w = "xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"";
    let styles = format!(
        "<w:styles {w}><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/></w:style></w:styles>"
    );
    let numbering = format!(
        "<w:numbering {w}><w:abstractNum w:abstractNumId=\"0\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"bullet\"/></w:lvl></w:abstractNum>\
         <w:abstractNum w:abstractNumId=\"1\"><w:lvl w:ilvl=\"0\"><w:numFmt w:val=\"decimal\"/></w:lvl></w:abstractNum>\
         <w:num w:numId=\"1\"><w:abstractNumId w:val=\"0\"/></w:num><w:num w:numId=\"2\"><w:abstractNumId w:val=\"1\"/></w:num></w:numbering>"
    );
    let rels = "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
        <Relationship Id=\"rId1\" Type=\"http://x/image\" Target=\"media/image1.png\"/>\
        <Relationship Id=\"rId2\" Type=\"http://x/hyperlink\" Target=\"https://example.org/a\" TargetMode=\"External\"/>\
        </Relationships>";
    paperpaw_zip::testing::ZipBuilder::new()
        .deflated(
            "word/document.xml",
            format!(
                "<?xml version=\"1.0\"?><w:document {WORD_NS}><w:body>{body}</w:body></w:document>"
            ),
        )
        .deflated("word/styles.xml", styles)
        .deflated("word/numbering.xml", numbering)
        .deflated("word/_rels/document.xml.rels", rels)
        .deflated(
            "word/media/image1.png",
            include_bytes!("../../../crates/paperpaw-image/tests/data/gray.png").to_vec(),
        )
        .build()
}

/// A long Word document: 25,000 paragraphs (about 4 MB of XML) with a
/// heading in each tenth, bold runs, links and bookmarks, 50 pictures,
/// lists and tables, and nesting of 200 levels twice.
fn long_word_document() -> Vec<u8> {
    let mut body = String::new();
    for i in 0..25_000 {
        if i % 10 == 0 {
            body.push_str(&format!(
                "<w:p><w:pPr><w:pStyle w:val=\"Heading1\"/></w:pPr><w:bookmarkStart w:id=\"{i}\" w:name=\"h{i}\"/><w:r><w:t>Chapter {i}</w:t></w:r></w:p>"
            ));
        }
        body.push_str("<w:p><w:r><w:rPr><w:b/></w:rPr><w:t>The wind came over the hills and over the sea, </w:t></w:r><w:r><w:t>and the old house stood quiet in the grey light.</w:t></w:r></w:p>");
        if i % 50 == 0 {
            body.push_str("<w:p><w:hyperlink r:id=\"rId2\"><w:r><w:t>a link</w:t></w:r></w:hyperlink><w:hyperlink w:anchor=\"h0\"><w:r><w:t> back</w:t></w:r></w:hyperlink></w:p>");
        }
        if i % 100 == 0 {
            for level in 0..4 {
                body.push_str(&format!(
                    "<w:p><w:pPr><w:numPr><w:ilvl w:val=\"{level}\"/><w:numId w:val=\"{}\"/></w:numPr></w:pPr><w:r><w:t>item {level}</w:t></w:r></w:p>",
                    1 + level % 2
                ));
            }
        }
        if i % 500 == 0 {
            body.push_str("<w:p><w:r><w:drawing><wp:inline><wp:docPr id=\"1\" name=\"P\" descr=\"A picture\"/><a:graphic><a:graphicData><a:blip r:embed=\"rId1\"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>");
        }
        if i % 700 == 0 {
            body.push_str("<w:tbl>");
            for _ in 0..3 {
                body.push_str("<w:tr><w:tc><w:p><w:r><w:t>one</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>two</w:t></w:r></w:p></w:tc></w:tr>");
            }
            body.push_str("</w:tbl>");
        }
    }
    body.push_str(&"<w:p>".repeat(200));
    body.push_str(&"</w:p>".repeat(200));
    body.push_str(&"<w:tbl><w:tr><w:tc>".repeat(200));
    body.push_str(&"</w:tc></w:tr></w:tbl>".repeat(200));
    word_document(&body)
}

/// Documents made to do harm: one paragraph of 3 MB with spaces and one of
/// 1 MB without, a hundred thousand bookmarks, runs and pictures in one
/// paragraph, and nesting of 100,000 levels.
fn hostile_word_document() -> Vec<u8> {
    let mut body = format!(
        "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
        "word ".repeat(600_000)
    );
    body.push_str(&format!(
        "<w:p><w:r><w:t>{}</w:t></w:r></w:p>",
        "x".repeat(1_000_000)
    ));
    body.push_str(&"<w:bookmarkStart w:name=\"a\"/>".repeat(100_000));
    body.push_str(&format!(
        "<w:p>{}</w:p>",
        "<w:r><w:rPr><w:b/></w:rPr><w:t>b</w:t></w:r><w:r><w:t>n</w:t></w:r>".repeat(50_000)
    ));
    body.push_str(&format!(
        "<w:p>{}</w:p>",
        "<w:r><w:drawing><a:blip r:embed=\"rId1\"/></w:drawing></w:r>".repeat(1000)
    ));
    body.push_str(&"<w:p>".repeat(100_000));
    body.push_str(&"</w:p>".repeat(100_000));
    word_document(&body)
}

/// A zip bomb: 24 MB of XML that is 30 KB deflated, in the archive.
fn bomb_word_document() -> Vec<u8> {
    word_document(&"<w:p><w:r><w:t>a</w:t></w:r></w:p>".repeat(700_000))
}

/// A Word document opened on the X4 (converted, then 30 pages forward and
/// back and home) stays within the budget, whatever it holds.
fn word_stays_within_budget(name: &str, document: Vec<u8>) -> Heap {
    let book = "books/document.docx";
    let heap = flow_stays_within_budget(
        name,
        "xteink-x4",
        book,
        &reading_flow(),
        &[(book, document)],
        &[],
    );
    eprintln!("{name}: peak {} KiB of {:?}", heap.peak, heap.limit);
    heap
}

#[test]
fn a_long_word_document_stays_within_the_x4_budget() {
    word_stays_within_budget("x4-docx-long", long_word_document());
}

#[test]
fn a_hostile_word_document_stays_within_the_x4_budget() {
    word_stays_within_budget("x4-docx-hostile", hostile_word_document());
}

#[test]
fn a_word_zip_bomb_stays_within_the_x4_budget() {
    word_stays_within_budget("x4-docx-bomb", bomb_word_document());
}

/// A folder of pictures read as a book on the X4: a JPEG, a PNG, a GIF, a
/// damaged JPEG, a file that is no picture and a JPEG again; 30 pages
/// forward and back and home. (A progressive JPEG of this size takes about
/// 2 MB to decode: the X4 refuses it, its page names the file; the run is
/// the same with and without the budget only where nothing is refused.)
#[test]
fn pictures_as_a_book_stay_within_the_x4_budget() {
    let data = root().join("crates/paperpaw-image/tests/data");
    let read = |name: &str| fs::read(data.join(name)).unwrap();
    let mut damaged = read("alice-cover.jpg");
    damaged.truncate(damaged.len() / 3);
    let files = [
        ("books/pictures/01.jpg", read("alice-cover.jpg")),
        ("books/pictures/02.png", read("colors-rgb.png")),
        ("books/pictures/03.gif", read("alice-cover.gif")),
        ("books/pictures/04.jpg", damaged),
        ("books/pictures/05.png", b"not a picture".to_vec()),
        ("books/pictures/06.jpg", read("alice-cover.jpg")),
    ];
    let heap = flow_stays_within_budget(
        "x4-pictures",
        "xteink-x4",
        "books/pictures/01.jpg",
        &reading_flow(),
        &files,
        &[],
    );
    eprintln!("pictures as a book: peak {} KiB", heap.peak);
}
