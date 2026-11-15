//! What a page turn costs in host crossings (#2936). On the X4 each crossing
//! from the reader into the host takes a millisecond or more, and 80 % of a
//! turn was spent inside them: the reader read its settings about 390 times
//! and crossed once per word in every band it was asked to draw. The turn
//! took 2.3 to 2.6 s, Beta 3's 0.3 s draw included.
//!
//! This counts the host slots a turn enters with the kernel's `slot-counts`
//! feature (the emulator counts like the device; its milliseconds are not the
//! device's, its call counts are). Ceilings are in `CEILINGS`; they sit above
//! what a reader that reads its setup once and draws each word once needs,
//! and far below what it did. One test only: the counts are per thread, and
//! this file is one binary.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::rc::Rc;

use paperpaw_core::native_host::host_slot::slot_counts;
use paperpaw_core::sdk::MemoryStorage;
use paperpaw_emulator::{device::Device, profiles};
use paperpaw_platform::{ButtonState, InputEvent, buttons};

/// `(text measures, all slots)`, the most of one turn, of the other cases of
/// `a_page_turn_stays_within_its_host_crossings`, as measured on the emulator
/// (the integration base's, which set no pair at all, in brackets): a page of
/// ideographs with more different ones than the table holds 2,457 (2,569: the
/// base's own is the ceiling, as nothing can be kept for most of them), a
/// Cyrillic book that starts in Latin 22 (116), a page of Vietnamese 238 (535),
/// kerning and ligatures off 151 (224).
const CHINESE: (u32, u32) = (2_569, 2_742);
const CYRILLIC: (u32, u32) = (40, 340);
const VIETNAMESE: (u32, u32) = (260, 560);
const PLAIN: (u32, u32) = (200, 560);

/// Turns that are not measured: the book opens and its caches fill over the
/// first turns (the first four after opening read 162, 125, 22 and 113 times
/// from the card, against 13 to 59 later; on the device the first turn is far
/// dearer still).
const WARM_UP: usize = 8;
/// Turns measured; each must stay under every ceiling.
const MEASURED: usize = 30;

/// `(slot, most entries one turn may make, why)`, for the sample page at the
/// default size (the worst of 30 turns on the reader of #1488 in brackets).
///
/// A word, and every other piece of text on the page, is drawn once per turn,
/// not once per band as before (654 inked draws, 413 settings reads and 1,437
/// entries in all on integration f19c3494b). The reader reads its setup when
/// it changes, not at a turn. The rest guards a slot that grows unseen.
const CEILINGS: &[(&str, u32, &str)] = &[
    ("settings_read", 5, "read once and refreshed on change (0)"),
    (
        "text_draw_inked",
        300,
        "one per piece of text, not per band (261)",
    ),
    (
        "text_measure",
        220,
        "layout of the next page, widths and pairs kept for the book (215)",
    ),
    (
        "storage_read_at",
        150,
        "card reads; a chapter's first page (87)",
    ),
    ("service_call", 30, "document service and status line (17)"),
];

/// All slots together, one turn (493 now).
const TOTAL_CEILING: u32 = 510;

/// Floors, so that a turn which does nothing cannot pass: every measured turn
/// enters at least this many slots (the quietest, a page with one picture,
/// made 100), and the turns together draw at least this many inked pieces of
/// text per turn on average (about 205 measured).
const TURN_FLOOR: u32 = 50;
const INKED_PER_TURN_FLOOR: u32 = 100;

fn press_button(device: &mut Device, name: &'static str, state: ButtonState) {
    device.call(|kernel, now| kernel.handle(now, InputEvent::Button { name, state }));
    device.advance(400);
}

fn press(device: &mut Device, state: ButtonState) {
    press_button(device, buttons::PAGE_FORWARD, state);
}

/// A device on the reader with `book` (of this `name`) open, `ini` more settings.
fn reading(name: &str, book: &[u8], ini: &str) -> Device {
    let files = MemoryStorage::new()
        .with(&format!("books/{name}"), book)
        .with(".paperpaw/plugins/setup/done", "")
        .with(
            ".paperpaw/plugins/reader/state.ini",
            format!("[reader]\nlast = books/{name}\n"),
        )
        .with("paperpaw.ini", format!("[help]\nshow = false\n{ini}"));
    paperpaw_emulator::budget::override_limit(None);
    let profile = profiles::find("xteink-x4").unwrap();
    let mut device = Device::in_memory(profile, None, Rc::new(files), true, None).unwrap();
    device.start();
    paperpaw_emulator::headless::run_script(&mut device, "wait:3000").unwrap();
    let active = |device: &mut Device| {
        let mut screen = None;
        device.call(|kernel, _| {
            screen = kernel.active_screen().map(ToString::to_string);
            paperpaw_core::Effects::default()
        });
        screen
    };
    // A book in a script the firmware has no letters of asks first whether to
    // fetch them: Back says no, and the book is there.
    if active(&mut device).as_deref() == Some("core.confirm-dialog") {
        press_button(&mut device, buttons::BACK, ButtonState::Pressed);
        press_button(&mut device, buttons::BACK, ButtonState::Released);
    }
    assert_eq!(active(&mut device).as_deref(), Some("reader.screen"));
    device
}

/// What the measured turns of a book entered: the most of each slot of
/// `CEILINGS`, of all together, the fewest of a turn, and the inked draws.
struct Seen {
    worst: Vec<u32>,
    total: u32,
    quietest: u32,
    inked: u32,
}

/// Turns `MEASURED` pages (after `WARM_UP`) of `book` and counts the host slots.
fn measure(label: &str, name: &str, book: &[u8], ini: &str) -> Seen {
    let mut device = reading(name, book, ini);
    for _ in 0..WARM_UP {
        press(&mut device, ButtonState::Pressed);
        press(&mut device, ButtonState::Released);
    }
    let mut seen = Seen {
        worst: vec![0; CEILINGS.len()],
        total: 0,
        quietest: u32::MAX,
        inked: 0,
    };
    for _ in 0..MEASURED {
        slot_counts::reset();
        press(&mut device, ButtonState::Pressed);
        press(&mut device, ButtonState::Released);
        let counts = slot_counts::snapshot();
        eprintln!(
            "page turn {label}: measures {} reads {} service {} settings {} inked {}",
            count_of(&counts, "text_measure"),
            count_of(&counts, "storage_read_at"),
            count_of(&counts, "service_call"),
            count_of(&counts, "settings_read"),
            count_of(&counts, "text_draw_inked"),
        );
        let turn: u32 = counts.iter().map(|(_, n)| *n).sum();
        seen.total = seen.total.max(turn);
        seen.quietest = seen.quietest.min(turn);
        seen.inked += count_of(&counts, "text_draw_inked");
        for (at, (slot, ..)) in CEILINGS.iter().enumerate() {
            seen.worst[at] = seen.worst[at].max(count_of(&counts, slot));
        }
    }
    seen
}

fn count_of(counts: &[(&'static str, u32)], slot: &str) -> u32 {
    counts
        .iter()
        .find(|(name, _)| *name == slot)
        .map_or(0, |(_, n)| *n)
}

/// A book of paragraphs of ideographs (2,500 different ones, so a page has more
/// than the 256 the table of characters after the alphabet holds,
/// with the ideographic comma and full stop), 400 characters each.
fn chinese_book() -> Vec<u8> {
    let mut seed = 12_345_u32;
    let mut next = |most: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) % most
    };
    let mut html = String::from("<html><head><title>Chinese</title></head><body>");
    for _ in 0..80 {
        html.push_str("<p>");
        for n in 0..400 {
            let c = match n % 25 {
                24 => '\u{3002}',
                12 => '\u{3001}',
                _ => char::from_u32(0x4e00 + next(2500)).unwrap(),
            };
            html.push(c);
        }
        html.push_str("</p>");
    }
    html.push_str("</body></html>");
    html.into_bytes()
}

/// A Latin specimen that takes the first 96 characters the reader numbers (all
/// of ASCII and of the Latin-1 letters), then pages of Cyrillic.
fn cyrillic_after_latin_book() -> Vec<u8> {
    let mut seed = 54_321_u32;
    let mut next = |most: u32| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        (seed >> 16) % most
    };
    let mut html = String::from("<html><head><title>Cyrillic</title></head><body><p>");
    for c in (33..127u8).map(char::from) {
        match c {
            '<' => html.push_str("&lt;"),
            '>' => html.push_str("&gt;"),
            '&' => html.push_str("&amp;"),
            c => html.push(c),
        }
    }
    html.push(' ');
    html.extend((0xc0..0x100u32).filter_map(char::from_u32));
    html.push_str("</p>");
    for _ in 0..80 {
        html.push_str("<p>");
        for _ in 0..70 {
            for _ in 0..2 + next(8) {
                html.push(char::from_u32(0x430 + next(32)).unwrap());
            }
            html.push_str(if next(9) == 0 { ", " } else { " " });
        }
        html.push_str(".</p>");
    }
    html.push_str("</body></html>");
    html.into_bytes()
}

/// Vietnamese: about 130 different letters (with a mark on a vowel or two), more
/// than the first 96 characters a book numbers.
fn vietnamese_book() -> Vec<u8> {
    let vowels: Vec<char> =
        "aàáảãạăằắẳẵặâầấẩẫậeèéẻẽẹêềếểễệiìíỉĩịoòóỏõọôồốổỗộơờớởỡợuùúủũụưừứửữựyỳýỷỹỵ"
            .chars()
            .collect();
    let consonants: Vec<char> = "bcdđghklmnpqrstvxBCDĐGHKLMNPQRSTVX".chars().collect();
    let mut seed = 98_765_u32;
    let mut next = |most: usize| {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        ((seed >> 16) as usize) % most
    };
    let mut html = String::from("<html><head><title>Vietnamese</title></head><body>");
    for _ in 0..80 {
        html.push_str("<p>");
        for _ in 0..90 {
            for n in 0..2 + next(5) {
                let from = if n % 2 == 0 { &consonants } else { &vowels };
                html.push(from[next(from.len())]);
            }
            html.push(' ');
        }
        html.push_str(".</p>");
    }
    html.push_str("</body></html>");
    html.into_bytes()
}

/// The Latin book at the default settings: every slot under its ceiling, and
/// the turns did something.
fn latin_over(alice: &[u8], over: &mut Vec<String>) {
    let seen = measure("alice", "alice-in-wonderland.epub", alice, "");
    for ((slot, ceiling, why), seen) in CEILINGS.iter().zip(&seen.worst) {
        eprintln!("page turn: {slot}: most {seen} (ceiling {ceiling}, {why})");
        if seen > ceiling {
            over.push(format!("{slot}: {seen} > {ceiling} ({why})"));
        }
    }
    eprintln!(
        "page turn: all slots together: most {} (ceiling {TOTAL_CEILING})",
        seen.total
    );
    if seen.total > TOTAL_CEILING {
        over.push(format!(
            "all slots together: {} > {TOTAL_CEILING}",
            seen.total
        ));
    }
    if seen.quietest < TURN_FLOOR {
        over.push(format!(
            "a turn made only {} slot entries, under {TURN_FLOOR}: it did nothing",
            seen.quietest
        ));
    }
    if seen.inked < INKED_PER_TURN_FLOOR * MEASURED as u32 {
        over.push(format!(
            "{} inked draws in {MEASURED} turns, under {} : the pages were not drawn",
            seen.inked,
            INKED_PER_TURN_FLOOR * MEASURED as u32
        ));
    }
}

/// A book (or settings) the Latin page does not show, with the most text
/// measures and all slots one turn of it may enter.
struct Case<'a> {
    label: &'a str,
    name: &'a str,
    book: &'a [u8],
    ini: &'a str,
    ceilings: (u32, u32),
}

/// What a book of another script, a book that starts in Latin and goes on in
/// Cyrillic, Vietnamese and a reader with kerning and ligatures off cost.
fn cases_over(alice: &[u8], over: &mut Vec<String>) {
    let (chinese, cyrillic, vietnamese) = (
        chinese_book(),
        cyrillic_after_latin_book(),
        vietnamese_book(),
    );
    let case = |label, name, book, ini, ceilings| Case {
        label,
        name,
        book,
        ini,
        ceilings,
    };
    let cases = [
        case("chinese", "chinese.html", &chinese[..], "", CHINESE),
        case("cyrillic", "cyrillic.html", &cyrillic[..], "", CYRILLIC),
        case(
            "vietnamese",
            "vietnamese.html",
            &vietnamese[..],
            "",
            VIETNAMESE,
        ),
        case(
            "plain",
            "alice-in-wonderland.epub",
            alice,
            "[core]\nkerning = off\nligatures = off\n",
            PLAIN,
        ),
    ];
    let measures = CEILINGS
        .iter()
        .position(|(slot, ..)| *slot == "text_measure");
    for Case {
        label,
        name,
        book,
        ini,
        ceilings: (measure_ceiling, total_ceiling),
    } in cases
    {
        let seen = measure(label, name, book, ini);
        let measured = seen.worst[measures.unwrap()];
        eprintln!(
            "page turn {label}: measures most {measured} (ceiling {measure_ceiling}), \
             all slots most {} (ceiling {total_ceiling})",
            seen.total
        );
        if measured > measure_ceiling {
            over.push(format!("{label}: measures {measured} > {measure_ceiling}"));
        }
        if seen.total > total_ceiling {
            over.push(format!(
                "{label}: all slots {} > {total_ceiling}",
                seen.total
            ));
        }
        if seen.quietest < TURN_FLOOR {
            over.push(format!(
                "{label}: a turn made only {} slot entries",
                seen.quietest
            ));
        }
    }
}

#[test]
fn a_page_turn_stays_within_its_host_crossings() {
    let alice = include_bytes!("../../../samples/library/books/alice-in-wonderland.epub");
    let mut over = Vec::new();
    latin_over(alice, &mut over);
    cases_over(alice, &mut over);
    assert!(
        over.is_empty(),
        "a page turn crosses into the host too often: {over:#?}"
    );
}
