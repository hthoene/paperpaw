//! How many button presses texts take on the X4 (buttons only, at the
//! default size): the keyboard before (its rows split, "qwert" above
//! "yuiop", capitals by *Shift* alone) against now (a computer keyboard's
//! rows, the column kept between rows, a capital also by holding the
//! letter). Each letter by the fewest presses: moves, then Select (a long
//! press counts as one).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::collections::{HashMap, VecDeque};

use paperpaw_core::{Effects, Kernel};
use paperpaw_platform::{
    DeviceInfo, DisplayInfo, InputCaps, Instant, MemoryClass, PixelFormat, Rotation,
};
use paperpaw_plugin_keyboard::Keyboard;
use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::{Part, Role};
use paperpaw_sdk::{Id, MemoryStorage, Params};

/// The X4: buttons only.
const X4: DeviceInfo = DeviceInfo::new(
    "test-x4",
    "Test X4",
    DisplayInfo::new(480, 800, 217, PixelFormat::Mono1, Rotation::Deg0, None),
    InputCaps::new(
        true,
        false,
        false,
        &["back", "confirm", "left", "right", "up", "down", "power"],
    ),
    MemoryClass::Small,
);

/// A key: its label and where it is across (left, width).
type Key = (String, u32, u32);

/// The keyboard on the X4 before, at Medium: its rows as drawn.
fn before() -> Vec<Vec<Key>> {
    let row = |keys: &[(&str, u32, u32)]| -> Vec<Key> {
        keys.iter()
            .map(|(label, x, w)| ((*label).to_string(), *x, *w))
            .collect()
    };
    let five = |labels: [&str; 5]| {
        row(&[
            (labels[0], 34, 68),
            (labels[1], 120, 68),
            (labels[2], 206, 68),
            (labels[3], 292, 68),
            (labels[4], 378, 68),
        ])
    };
    vec![
        five(["q", "w", "e", "r", "t"]),
        five(["y", "u", "i", "o", "p"]),
        row(&[
            ("a", 34, 29),
            ("s", 81, 30),
            ("d", 129, 30),
            ("f", 177, 30),
            ("g", 225, 29),
            ("h", 272, 30),
            ("j", 320, 30),
            ("k", 368, 30),
            ("l", 416, 30),
        ]),
        row(&[
            ("Shift", 34, 99),
            ("z", 151, 60),
            ("x", 229, 60),
            ("c", 307, 60),
            ("v", 385, 61),
        ]),
        row(&[
            ("b", 34, 77),
            ("n", 129, 78),
            ("m", 225, 77),
            ("Delete", 320, 126),
        ]),
        row(&[
            ("123", 34, 46),
            (",", 98, 25),
            ("Space", 141, 197),
            (".", 356, 25),
            ("OK", 399, 47),
        ]),
    ]
}

/// The keyboard on the X4 now, at the default size: its rows as drawn.
fn now() -> Vec<Vec<Key>> {
    let mut kernel = Kernel::builder(X4)
        .storage(MemoryStorage::new())
        .user_config("[core]\nlanguage = en\n[help]\nshow = false\n".to_string())
        .plugin(paperpaw_plugin_fonts_default::DefaultFontPacks)
        .plugin(Keyboard)
        .build()
        .unwrap();
    let mut effects = kernel.start(Instant(0));
    settle(&mut kernel, &mut effects);
    let params = Params::new().with(params::TITLE, "Name");
    let mut effects = kernel.open_slot(Instant(100), slots::KEYBOARD, params);
    settle(&mut kernel, &mut effects);
    assert_eq!(
        kernel.active_screen().map(Id::as_str),
        Some("keyboard.screen")
    );
    kernel.render(&mut |_, _| {});
    let mut keys: Vec<Part> = kernel
        .describe()
        .into_iter()
        .filter(|p| p.role == Role::Button)
        .collect();
    keys.sort_by_key(|p| (p.area.y, p.area.x));
    let mut rows: Vec<Vec<Key>> = Vec::new();
    let mut top = None;
    for part in keys {
        if top != Some(part.area.y) {
            rows.push(Vec::new());
            top = Some(part.area.y);
        }
        let key = (
            part.label.clone(),
            u32::from(part.area.x),
            u32::from(part.area.width),
        );
        rows.last_mut().unwrap().push(key);
    }
    rows
}

fn settle(kernel: &mut Kernel, effects: &mut Effects) {
    let mut now = 0;
    while let Some(at) = effects.wake_at() {
        if at.0 > now + 1000 {
            return;
        }
        now = now.max(at.0);
        *effects = kernel.tick(Instant(now));
    }
}

/// Where the focus is: a key, and the column it keeps (now only).
type Focus = (usize, usize, u32);

/// How the focus moves on `rows`: left and right round a row, up and
/// down to the key of the next row (round from the last to the first)
/// below the column (`keep`: the one kept since the last move along a
/// row, else the middle of the key).
struct Moves<'a> {
    rows: &'a [Vec<Key>],
    keep: bool,
}

impl Moves<'_> {
    fn center(&self, row: usize, index: usize) -> u32 {
        let (_, x, w) = &self.rows[row][index];
        x + w / 2
    }

    fn below(&self, to: usize, column: u32) -> usize {
        let keys = &self.rows[to];
        (0..keys.len())
            .find(|&i| keys[i].1 + keys[i].2 > column)
            .unwrap_or(keys.len() - 1)
    }

    fn next(&self, (row, index, column): Focus) -> [Focus; 4] {
        let rows = self.rows.len();
        let len = self.rows[row].len();
        let left = (index + len - 1) % len;
        let right = (index + 1) % len;
        let column = if self.keep {
            column
        } else {
            self.center(row, index)
        };
        let up = (row + rows - 1) % rows;
        let down = (row + 1) % rows;
        [
            (row, left, self.center(row, left)),
            (row, right, self.center(row, right)),
            (up, self.below(up, column), column),
            (down, self.below(down, column), column),
        ]
    }

    /// The fewest moves from `from` to the key labelled `to`, and where
    /// the focus then is.
    fn path(&self, from: Focus, to: &str) -> (usize, Focus) {
        let mut seen: HashMap<Focus, usize> = HashMap::from([(from, 0)]);
        let mut queue = VecDeque::from([from]);
        while let Some(at) = queue.pop_front() {
            if self.rows[at.0][at.1].0 == to {
                return (seen[&at], at);
            }
            for next in self.next(at) {
                if !seen.contains_key(&next) {
                    seen.insert(next, seen[&at] + 1);
                    queue.push_back(next);
                }
            }
        }
        panic!("no key {to}");
    }
}

/// The presses `text` takes on `rows`; `hold` for a capital by holding
/// its letter (else by *Shift*). The focus starts on `q`.
fn presses(rows: &[Vec<Key>], keep: bool, hold: bool, text: &str) -> usize {
    let moves = Moves { rows, keep };
    let mut at: Focus = (0, 0, moves.center(0, 0));
    let mut count = 0;
    for c in text.chars() {
        let label = match c {
            ' ' => "Space".to_string(),
            c => c.to_lowercase().to_string(),
        };
        let capital = c.is_uppercase();
        let (to_letter, on_letter) = moves.path(at, &label);
        if !capital {
            count += to_letter + 1;
            at = on_letter;
            continue;
        }
        // By Shift: there, Select, to the letter, Select.
        let (to_shift, on_shift) = moves.path(at, "Shift");
        let (shift_to_letter, after) = moves.path(on_shift, &label);
        let by_shift = to_shift + 1 + shift_to_letter + 1;
        // By holding the letter: there, a long press, Select (the capital
        // has the focus).
        let by_hold = to_letter + 2;
        if hold && by_hold <= by_shift {
            count += by_hold;
            at = on_letter;
        } else {
            count += by_shift;
            at = after;
        }
    }
    count
}

const TEXTS: [&str; 4] = [
    "The quick brown fox jumps over the lazy dog.",
    "Meet me at the station at noon, Anna.",
    "Pride and Prejudice by Jane Austen",
    "My Wi Fi is called Garden and the key is long",
];

/// The letters of the computer keyboard's rows in a row each, and all
/// sample texts together take no more presses than before: the wider
/// rows cost some, holding a letter for its capital saves more. The
/// numbers are printed (`--nocapture`).
#[test]
fn texts_take_no_more_presses_than_before() {
    let old = before();
    let new = now();
    let letters: Vec<String> = new
        .iter()
        .take(3)
        .map(|row| row.iter().map(|k| k.0.as_str()).collect::<String>())
        .collect();
    assert_eq!(letters, ["qwertyuiop", "asdfghjkl", "ShiftzxcvbnmDelete"]);
    let (mut all_old, mut all_new, mut all_chars) = (0, 0, 0);
    for text in TEXTS {
        let was = presses(&old, false, false, text);
        let is = presses(&new, true, true, text);
        let chars = text.chars().count();
        println!(
            "{text:?}: {was} presses before, {is} now ({:.1} and {:.1} a character)",
            was as f32 / chars as f32,
            is as f32 / chars as f32
        );
        (all_old, all_new, all_chars) = (all_old + was, all_new + is, all_chars + chars);
    }
    println!(
        "all: {all_old} before, {all_new} now, {:.2} and {:.2} a character",
        all_old as f32 / all_chars as f32,
        all_new as f32 / all_chars as f32
    );
    assert!(all_new <= all_old, "{all_new} now, {all_old} before");
    // What each change gives: holding a letter for its capital, keeping
    // the column (it costs a few presses, and keeps up and down where one
    // expects them).
    println!(
        "now without holding for capitals: {}; without the column kept: {}",
        TEXTS
            .iter()
            .map(|t| presses(&new, true, false, t))
            .sum::<usize>(),
        TEXTS
            .iter()
            .map(|t| presses(&new, false, true, t))
            .sum::<usize>(),
    );
}
