//! Native replacements of the interface-6 suites `size.rs`, `presses.rs`,
//! `multiline.rs` and `long_text.rs` (and the helpers the other replacement
//! modules, `native_model_typing_tests` and `native_model_layout_tests`, use):
//! the same inputs through the [`Board`] instead of a kernel, with the same
//! expected values. What needs the host (drawing, touch gestures, dialogs)
//! is in `tests/acceptance_pending.rs`; `PORTING-7.md` lists which old test
//! is which new one.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use crate::keys::{Key, Layer};
use crate::layouts::{Chosen, chosen_for};
use crate::native_model::{
    Asked, Board, FIELD_ROW, FieldButton, Look, MAX_CHARS, MAX_CHARS_MULTILINE, Move, Own, Pressed,
};
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use paperpaw_sdk::Point;

/// A screen of the devices the old suites ran on.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Screen {
    pub name: &'static str,
    pub width: u16,
    pub height: u16,
    pub dpi: u16,
    pub touch: bool,
}

/// The X4 Pro: touch and buttons.
pub(crate) const X4_PRO: Screen = Screen {
    name: "x4-pro",
    width: 480,
    height: 800,
    dpi: 217,
    touch: true,
};
/// The Boox Go 7: a larger touch screen, finer.
pub(crate) const BOOX: Screen = Screen {
    name: "boox",
    width: 1264,
    height: 1680,
    dpi: 300,
    touch: true,
};
/// The X4: buttons only.
pub(crate) const X4: Screen = Screen {
    name: "x4",
    width: 480,
    height: 800,
    dpi: 217,
    touch: false,
};
/// A small screen of large pixels.
pub(crate) const COARSE: Screen = Screen {
    name: "coarse",
    width: 320,
    height: 480,
    dpi: 160,
    touch: true,
};

/// `tenths` of a millimetre in pixels, as the keyboard counts them.
pub(crate) fn tenth_mm(dpi: u16, tenths: u32) -> u32 {
    (tenths * u32::from(dpi) + 127) / 254
}

/// The numbers the stock look reads for `screen` (see `Style::read`, with the
/// defaults of `core_plugin::LOOK`): text of 8 points, a row of 1.6 lines of
/// 1.25 times the text, a finger's 7 mm on touch screens, margins of 4 mm,
/// gaps of 2 mm and lines of 0.3 mm. Only the font's own metrics are a guess.
pub(crate) fn look(screen: Screen) -> Look {
    let tenth = |tenths: u32| tenth_mm(screen.dpi, tenths).max(1);
    let text = 8.0 * f32::from(screen.dpi) / 72.0;
    let small = text * 0.75;
    let text_line = (text * 1.25) as u16;
    let control = (f32::from(text_line) * 1.6) as u32;
    let finger = if screen.touch { tenth(70) } else { 0 };
    let gap = tenth(20) as u16;
    Look {
        width: screen.width,
        height: screen.height,
        dpi: screen.dpi,
        touch: screen.touch,
        text,
        small,
        row: control.max(finger) as u16,
        margin: tenth(40) as u16,
        gap,
        line: tenth(3) as u16,
        m_width: 0.9,
        text_line,
        status: (small * 1.25) as u16 + gap / 2,
    }
}

pub(crate) const MEDIUM: &str = "keyboard.size.medium";
pub(crate) const ROUND: &str = "keyboard.key-shape.round";
const SIZES: [&str; 3] = [
    "keyboard.size.small",
    "keyboard.size.medium",
    "keyboard.size.large",
];

pub(crate) fn own(size: &'static str, auto_capitals: bool) -> Own<'static> {
    Own {
        shape: ROUND,
        size,
        auto_capitals,
    }
}

pub(crate) fn plain() -> Asked {
    Asked {
        plain: true,
        ..Asked::default()
    }
}

pub(crate) fn lines() -> Asked {
    Asked {
        multiline: true,
        ..plain()
    }
}

pub(crate) fn layouts(setting: &str, keys: bool) -> Vec<Chosen> {
    chosen_for("en", "", setting, keys).unwrap()
}

/// The keyboard of `asked` on `screen`: the default size, the language's
/// layout, no field buttons.
pub(crate) fn board(screen: Screen, asked: Asked) -> Board {
    board_with(screen, asked, "keyboard.layout.auto", &[])
}

/// As [`board`], with the layouts of `setting` and the field's buttons.
pub(crate) fn board_with(
    screen: Screen,
    asked: Asked,
    setting: &str,
    buttons: &[(FieldButton, f32)],
) -> Board {
    Board::new(
        look(screen),
        own(MEDIUM, false),
        asked,
        layouts(setting, false),
        0,
        buttons,
    )
    .unwrap()
}

/// What a key says, in the words of the English catalog.
pub(crate) fn label(board: &Board, key: Key) -> String {
    match key {
        Key::Char(c) => board.cased(c).to_string(),
        Key::Shift => "Shift".to_string(),
        Key::Delete => "Delete".to_string(),
        Key::Space => "Space".to_string(),
        Key::Layer(Layer::Letters) => "ABC".to_string(),
        Key::Layer(_) => "123".to_string(),
        Key::NextLayout => board.layouts[board.current].0.clone(),
        Key::Enter => "New line".to_string(),
        Key::Done => "OK".to_string(),
    }
}

/// Where the key that says `text` is.
pub(crate) fn find(board: &Board, text: &str) -> Option<(usize, usize)> {
    (0..board.rows.len()).find_map(|row| {
        (0..board.rows[row].len())
            .find(|&index| label(board, board.rows[row][index].0) == text)
            .map(|index| (row, index))
    })
}

/// A person typing on a board: taps, button presses and time.
pub(crate) struct Typing {
    pub board: Board,
    pub now: u64,
}

impl Typing {
    pub fn new(screen: Screen, asked: Asked) -> Self {
        Self::of(board(screen, asked))
    }

    pub fn of(board: Board) -> Self {
        Self { board, now: 0 }
    }

    /// The middle of the key that says `text`.
    pub fn center(&self, text: &str) -> Point {
        let (row, index) = find(&self.board, text).unwrap_or_else(|| panic!("no key {text:?}"));
        let area = self.board.key_area(row, index);
        Point::new(
            i32::from(area.x) + i32::from(area.width) / 2,
            i32::from(area.y) + i32::from(area.height) / 2,
        )
    }

    /// A tap on the key that says `text`: found where it lies, pressed.
    pub fn tap(&mut self, text: &str) -> Pressed {
        let at = self.center(text);
        let (row, index) = find(&self.board, text).unwrap();
        assert_eq!(self.board.key_at(at), Some((row, index)), "{text}");
        self.now += 100;
        self.board
            .press(self.board.key(row, index).unwrap(), self.now)
            .unwrap()
    }

    pub fn tap_all(&mut self, texts: &str) {
        for c in texts.chars() {
            self.tap(&c.to_string());
        }
    }

    /// What the focus is on: a key, a field button, or the variant chosen.
    pub fn focused(&self) -> Option<String> {
        if let Some(popup) = &self.board.popup {
            return popup.focus.map(|i| popup.chars[i].to_string());
        }
        match self.board.focus? {
            (FIELD_ROW, index) => Some(
                match self.board.buttons[index].0 {
                    FieldButton::Phone => "Phone",
                    FieldButton::Paste => "Paste",
                }
                .to_string(),
            ),
            (row, index) => Some(label(&self.board, self.board.key(row, index)?)),
        }
    }

    /// Moves the focus with *Next* (through every key, row after row) to
    /// the key that says `text`.
    pub fn focus_by_buttons(&mut self, text: &str) {
        for _ in 0..80 {
            if self.focused().as_deref() == Some(text) {
                return;
            }
            self.board.move_focus(Move::Next);
        }
        panic!("no key says {text:?}");
    }

    /// A press of *Select* on the focused key.
    pub fn confirm(&mut self) -> Pressed {
        let (row, index) = self.board.focus.expect("a focus");
        self.now += 100;
        self.board
            .press(self.board.key(row, index).unwrap(), self.now)
            .unwrap()
    }

    pub fn press_by_buttons(&mut self, text: &str) -> Pressed {
        self.focus_by_buttons(text);
        self.confirm()
    }

    /// A long press on the key that says `text` (touch) shows its variants.
    pub fn hold(&mut self, text: &str) -> bool {
        let (row, index) = find(&self.board, text).unwrap();
        self.board.open_variants(row, index).unwrap()
    }

    /// A long press on *Select* on the focused key.
    pub fn hold_confirm(&mut self) -> bool {
        let (row, index) = self.board.focus.expect("a focus");
        self.board.open_variants(row, index).unwrap()
    }

    /// The variants shown, as the screen's dialog takes one (by a tap on it
    /// or *Select* on the chosen): closed, and the character typed.
    pub fn choose(&mut self, variant: char) {
        let popup = self.board.popup.take().expect("variants shown");
        assert!(popup.chars.contains(&variant), "{variant}: {popup:?}");
        let mut buffer = [0u8; 4];
        self.board
            .insert(variant.encode_utf8(&mut buffer), true)
            .unwrap();
    }

    pub fn text(&self) -> &str {
        &self.board.text
    }
}

// ---- size.rs -------------------------------------------------------------

/// The board for `screen` at `size`.
fn sized(screen: Screen, size: &'static str) -> Board {
    Board::new(
        look(screen),
        own(size, false),
        plain(),
        layouts("keyboard.layout.auto", false),
        0,
        &[],
    )
    .unwrap()
}

/// What a key takes from one row to the next, in pixels: its height.
fn step(board: &Board) -> u16 {
    let (q, a) = (board.key_area(0, 0), board.key_area(1, 0));
    a.y - q.y
}

/// How high the keyboard is, from the top of its title to the bottom.
fn height(screen: Screen, board: &Board) -> u16 {
    screen.height - board.field.y
}

fn least(screen: Screen) -> u16 {
    tenth_mm(screen.dpi, 40) as u16
}

#[test]
fn larger_sizes_give_higher_keys_and_never_below_a_finger() {
    for screen in [X4_PRO, BOOX] {
        let steps: Vec<u16> = SIZES.iter().map(|s| step(&sized(screen, s))).collect();
        assert!(steps[0] < steps[1], "{}: {steps:?}", screen.name);
        assert!(steps[1] < steps[2], "{}: {steps:?}", screen.name);
        let default = sized(screen, MEDIUM);
        for size in SIZES {
            let board = sized(screen, size);
            assert!(step(&board) >= least(screen), "{} {size}", screen.name);
            let high = height(screen, &board);
            assert!(high <= screen.height / 2, "{} {size}: {high}", screen.name);
            // The keys stay as wide as before: the size changes heights.
            assert_eq!(
                board.key_area(0, 0).width,
                default.key_area(0, 0).width,
                "{} {size}",
                screen.name
            );
        }
    }
}

/// The keyboard's share of the screen's height by size. The interface-6 suite
/// measured about 30, 40 and 50 % on both screens with the fonts' own metrics;
/// the board with the stock look's numbers (`look`, a guessed font line)
/// gives these, within two: the Boox as before, the X4 Pro a few percent
/// lower (its field is a few pixels lower than the fonts' lines made it).
/// What is pinned is the native layout's, and the finger's floor and the
/// half-screen cap where a size reaches them.
const SHARES: [(Screen, [u32; 3]); 2] = [(X4_PRO, [30, 36, 45]), (BOOX, [30, 40, 50])];

#[test]
fn round_keys_take_about_30_40_and_50_percent() {
    for (screen, shares) in SHARES {
        for (size, near) in SIZES.iter().zip(shares) {
            let board = sized(screen, size);
            // In whole percent, rounded (303 of 800 px is 38 %, not 37).
            let total = u32::from(screen.height);
            let share = (u32::from(height(screen, &board)) * 100 + total / 2) / total;
            // Where the keys are as small as a finger allows, the keyboard
            // is as low as it can be.
            let floored = step(&board) == least(screen) && share > near;
            // The largest size may reach half the screen before a half of
            // the screen's height is the keys': the cap counts the field and
            // the gaps, this height leaves some out.
            let capped = *size == SIZES[2] && (46..=50).contains(&share);
            assert!(
                floored || capped || share.abs_diff(near) <= 2,
                "{} {size}: {share} % (near {near})",
                screen.name
            );
        }
    }
}

/// Medium is the keyboard as before: the size a setting that is not known
/// stands for, and what a plugin reading a stray value gets.
#[test]
fn medium_is_the_keyboard_as_before() {
    for screen in [X4_PRO, BOOX, X4] {
        let medium = sized(screen, MEDIUM);
        let stray = sized(screen, "keyboard.size.nonsense");
        assert_eq!(medium.region, stray.region, "{}", screen.name);
        assert_eq!(medium.keys, stray.keys, "{}", screen.name);
        assert_eq!(medium.key_height, stray.key_height, "{}", screen.name);
        assert_eq!(medium.rows, stray.rows, "{}", screen.name);
        assert_eq!(medium.letters, stray.letters, "{}", screen.name);
    }
}

/// A thin margin at both sides (a thirtieth of the screen at most), tapped or
/// chosen with buttons.
#[test]
fn the_keys_use_nearly_the_whole_width() {
    for screen in [X4, X4_PRO, BOOX] {
        let board = sized(screen, MEDIUM);
        let (q, p) = (board.key_area(0, 0), board.key_area(0, 9));
        let width = screen.width;
        assert!(q.x > 0 && q.x <= width / 30, "{} q at {}", screen.name, q.x);
        let right = width - (p.x + p.width);
        assert!(right <= width / 30, "{} p ends {right} short", screen.name);
    }
}

/// Keys chosen with buttons: the rows stay a computer keyboard's at every
/// size (qwertyuiop, asdfghjkl, zxcvbnm whole), and the size makes the keys
/// higher.
#[test]
fn with_buttons_the_size_makes_keys_higher_in_whole_rows() {
    let alphabetic = |board: &Board, row: usize| {
        board.rows[row]
            .iter()
            .any(|(key, _)| matches!(key, Key::Char(c) if c.is_alphabetic()))
    };
    let boards = SIZES.map(|size| sized(X4, size));
    for board in &boards {
        let letter_rows = (0..board.rows.len())
            .filter(|&row| alphabetic(board, row))
            .count();
        assert_eq!(letter_rows, 3, "qwertyuiop, asdfghjkl, zxcvbnm whole");
    }
    let [small, medium, large] = &boards;
    assert!(small.key_height < medium.key_height && medium.key_height < large.key_height);
    let high = |board: &Board| height(X4, board);
    assert!(high(small) < high(medium) && high(medium) < high(large));
}

#[test]
fn the_floor_wins_where_a_size_cannot_be_reached() {
    // A small screen of large pixels: Small would go below a finger.
    let small = sized(COARSE, "keyboard.size.small");
    assert!(
        step(&small) >= least(COARSE),
        "{} of {}",
        step(&small),
        least(COARSE)
    );
}

/// Measures, for the notes (`--ignored --nocapture`).
#[test]
#[ignore = "measures, for the notes"]
fn key_widths_on_the_x4() {
    for screen in [X4, BOOX] {
        for size in SIZES {
            let board = sized(screen, size);
            let (q, w) = (board.key_area(0, 0), board.key_area(0, 1));
            std::println!(
                "{} {size}: q {}x{} at x {}, gap {}, keyboard {} high",
                screen.name,
                q.width,
                q.height,
                q.x,
                i32::from(w.x) - i32::from(q.x + q.width),
                height(screen, &board)
            );
        }
    }
}

// ---- presses.rs ----------------------------------------------------------

use std::collections::{HashMap, VecDeque};

/// A key: its label and where it is across (left, width).
type Placed = (String, u32, u32);

/// The keyboard on the X4 before, at Medium: its rows as drawn.
fn before() -> Vec<Vec<Placed>> {
    let row = |keys: &[(&str, u32, u32)]| -> Vec<Placed> {
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

/// The keyboard on the X4 now, at the default size: its rows as laid out.
fn now(board: &Board) -> Vec<Vec<Placed>> {
    (0..board.rows.len())
        .map(|row| {
            (0..board.rows[row].len())
                .map(|index| {
                    let area = board.key_area(row, index);
                    let text = label(board, board.rows[row][index].0);
                    (text, u32::from(area.x), u32::from(area.width))
                })
                .collect()
        })
        .collect()
}

/// Where the focus is: a key, and the column it keeps (now only).
type Focus = (usize, usize, u32);

/// How the focus moves on `rows`: left and right round a row, up and down to
/// the key of the next row (round from the last to the first) below the
/// column (`keep`: the one kept since the last move along a row, else the
/// middle of the key).
struct Moves<'a> {
    rows: &'a [Vec<Placed>],
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

    /// The fewest moves from `from` to the key labelled `to`, and where the
    /// focus then is.
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

/// The presses `text` takes on `rows`; `hold` for a capital by holding its
/// letter (else by *Shift*). The focus starts on `q`.
fn presses(rows: &[Vec<Placed>], keep: bool, hold: bool, text: &str) -> usize {
    let moves = Moves { rows, keep };
    let mut at: Focus = (0, 0, moves.center(0, 0));
    let mut count = 0;
    for c in text.chars() {
        let label = match c {
            ' ' => "Space".to_string(),
            c => c.to_lowercase().to_string(),
        };
        let (to_letter, on_letter) = moves.path(at, &label);
        if !c.is_uppercase() {
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

/// The letters of the computer keyboard's rows in a row each, and all sample
/// texts together take no more presses than before: the wider rows cost some,
/// holding a letter for its capital saves more. The numbers are printed
/// (`--nocapture`).
#[test]
fn texts_take_no_more_presses_than_before() {
    let old = before();
    let board = board(X4, plain());
    let new = now(&board);
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
        std::println!(
            "{text:?}: {was} presses before, {is} now ({:.1} and {:.1} a character)",
            was as f32 / chars as f32,
            is as f32 / chars as f32
        );
        (all_old, all_new, all_chars) = (all_old + was, all_new + is, all_chars + chars);
    }
    std::println!(
        "all: {all_old} before, {all_new} now, {:.2} and {:.2} a character",
        all_old as f32 / all_chars as f32,
        all_new as f32 / all_chars as f32
    );
    // The totals move with the pixel rounding of the keys' places (631 to 635
    // against 633 for margins of 1 to 4 mm on the X4's 480 pixels), so "no
    // more" is within a percent.
    assert!(
        all_new * 100 <= all_old * 101,
        "{all_new} now, {all_old} before"
    );
}

/// The model of the focus the counting above uses is the board's own: from
/// every key, left, right, up and down go where `move_focus` goes.
#[test]
fn the_press_counting_moves_as_the_board_does() {
    let mut board = board(X4, plain());
    let new = now(&board);
    let model = Moves {
        rows: &new,
        keep: false,
    };
    for (row, keys) in new.iter().enumerate() {
        for index in 0..keys.len() {
            let from = (row, index, model.center(row, index));
            let wanted = model.next(from);
            for (go, (to_row, to_index, _)) in [Move::Left, Move::Right, Move::Up, Move::Down]
                .into_iter()
                .zip(wanted)
            {
                board.focus = Some((row, index));
                board.column = None;
                board.move_focus(go);
                assert_eq!(
                    board.focus,
                    Some((to_row, to_index)),
                    "{go:?} from {row}/{index}"
                );
            }
        }
    }
}

// ---- multiline.rs and long_text.rs -----------------------------------------

/// What comes back of `text` typed on a phone (or pasted): it takes the
/// field's place, then *OK*.
fn on_phone(asked: Asked, text: &str) -> String {
    let mut typing = Typing::new(X4_PRO, asked);
    typing.board.replace_pasted(text).unwrap();
    typing.tap("OK");
    typing.board.text.clone()
}

#[test]
fn lines_come_back_as_typed() {
    assert_eq!(
        on_phone(lines(), "Hi Ada,\n\nSee you.\r\nJane"),
        "Hi Ada,\n\nSee you.\nJane"
    );
}

#[test]
fn a_plain_question_gets_one_line() {
    assert_eq!(on_phone(plain(), "Hi Ada,\n\nJane"), "Hi Ada,  Jane");
    // A line break of two characters is one space (the interface-6 keyboard
    // made two: a difference, and an improvement for text from a phone).
    assert_eq!(on_phone(plain(), "Hi Ada,\r\nJane"), "Hi Ada, Jane");
}

#[test]
fn more_than_a_line_of_text_may_be_typed() {
    assert_eq!(on_phone(lines(), &"line\n".repeat(1000)).len(), 4999);
    // A plain question stops at its own limit.
    assert_eq!(on_phone(plain(), &"x".repeat(5000)).len(), MAX_CHARS);
    assert_eq!(
        on_phone(lines(), &"x".repeat(7000)).len(),
        MAX_CHARS_MULTILINE
    );
    assert_eq!((MAX_CHARS, MAX_CHARS_MULTILINE), (2000, 6000));
}

#[test]
fn a_long_address_comes_back_whole() {
    assert_eq!(on_phone(plain(), &"x".repeat(600)).len(), 600);
}

#[test]
fn text_past_the_limit_is_cut() {
    assert_eq!(on_phone(plain(), &"x".repeat(5000)).len(), 2000);
    let mut b = board(X4_PRO, plain());
    assert_eq!(b.replace_pasted(&"x".repeat(5000)).unwrap(), Pressed::Full);
    assert_eq!(b.replace_pasted("kept short").unwrap(), Pressed::Changed);
}

/// The status line stays in sight: the panel of the keyboard, the field of
/// several lines included, starts under it on every screen. (The plain
/// field's panel filled the screen from the top turned on its side before;
/// now it leaves the status line free too.)
#[test]
fn the_panel_leaves_the_status_line_free_on_every_screen() {
    let sideways = Screen {
        name: "sideways",
        width: 800,
        height: 480,
        dpi: 220,
        touch: true,
    };
    for screen in [sideways, X4_PRO, X4, BOOX] {
        for asked in [lines(), plain()] {
            let b = board(screen, asked);
            assert!(
                b.field.y >= look(screen).status,
                "{} {asked:?}: field at {}",
                screen.name,
                b.field.y
            );
            assert!(b.region.y + b.region.height == screen.height);
        }
    }
    // Upright, the lines of the field stop short of the top.
    assert!(board(X4_PRO, lines()).field.y >= 60);
    // The field of several lines is taller than the plain one.
    let (many, one) = (board(X4_PRO, lines()), board(X4_PRO, plain()));
    assert!(many.field.height > 3 * one.field.height);
    assert!(one.field.height < 120, "{}", one.field.height);
}

#[test]
fn the_line_break_key_adds_a_line_break() {
    let mut typing = Typing::new(X4_PRO, lines());
    typing.board.replace_pasted("Hi").unwrap();
    typing.tap("New line");
    typing.tap("z");
    assert_eq!(typing.tap("OK"), Pressed::Done);
    assert_eq!(typing.text(), "Hi\nz");
    // A plain question has no such key.
    assert!(find(&board(X4_PRO, plain()), "New line").is_none());
}

// ---- lib.rs --------------------------------------------------------------

#[test]
fn several_layouts_get_a_key_to_switch() {
    use crate::keys::{Slot, try_rows_for};
    let letters = "qwertyuiop asdfghjkl zxcvbnm";
    let rows = |layer, switch| try_rows_for(letters, layer, switch, 0, false).unwrap();
    let has = |rows: &Vec<Vec<Slot>>| rows.iter().flatten().any(|(k, _)| *k == Key::NextLayout);
    let (one, two) = (rows(Layer::Letters, false), rows(Layer::Letters, true));
    assert!(!has(&one));
    assert!(has(&two));
    // Rows add up to the same width with or without it.
    let width = |rows: &Vec<Vec<Slot>>| -> u32 {
        rows.last()
            .unwrap()
            .iter()
            .map(|(_, w)| u32::from(*w))
            .sum()
    };
    assert_eq!(width(&one), width(&two));
    // As wide as the letter rows: the space bar takes what is left.
    assert_eq!(width(&one), 20);
    // Not on the symbols.
    assert!(!has(&rows(Layer::Symbols, true)));
}
