//! Memory on the small devices: the emulator charges what the kernel
//! and its plugins allocate against the profile's heap budget (256 KiB
//! on the X4, see `src/budget.rs`) and refuses what goes past it, as the
//! device would. Here the main flows run headless on the X4 profiles: a
//! book opened (EPUB and TXT), 30 pages forward and 30 back, home, the
//! library (with many books) opened and scrolled, the main menu, the
//! settings and a page of them; and the store, online with the
//! emulator's own catalogue.
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
    files: &[(&str, String)],
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
    files: &[(&str, String)],
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

/// The language catalogs are read in once the registries have given back
/// what they reserved, not before: German (longer texts than English)
/// starts within the peak of English, so a budget that fits English fits
/// it.
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
    assert!(
        german <= english + 0.05,
        "start: peak {german} KiB in German, {english} KiB in English"
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
fn bluetooth_stays_within_the_x4_pro_budget() {
    let mut secrets = paperpaw_core::sdk::secrets::Secrets::default();
    secrets.apply(paperpaw_core::sdk::secrets::SecretChange::keep(
        "core.bluetooth.1",
        "PaperPaw Remote",
        "fa:ke:00:00:00:01	PaperPaw Remote	remote	0		keys",
    ));
    let secrets = (paperpaw_core::sdk::secrets::FILE, secrets.encode());
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
    assert!(stdout.contains(notice), "{stdout}");
    let opened = heap(&free);
    assert_eq!(opened.screen, "reader.screen");
    assert!(opened.peak > budget.parse().unwrap(), "{opened:?}");
    assert!(!String::from_utf8_lossy(&free.stdout).contains(notice));
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
    // The sites: largest first, in PaperPaw's code.
    let sizes: Vec<f64> = stdout
        .lines()
        .skip_while(|l| !l.starts_with("live heap by call site"))
        .skip(1)
        .filter(|l| l.contains(" KiB "))
        .map(|l| l.split_whitespace().next().unwrap().parse().unwrap())
        .collect();
    assert!(sizes.len() > 3, "{stdout}");
    assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "{sizes:?}");
    assert!(stdout.contains("paperpaw_"), "{stdout}");
}
