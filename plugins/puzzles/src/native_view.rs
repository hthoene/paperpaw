//! The Puzzles screen's state and what a key or a tap means in it: the menu,
//! the lists, and the three boards (sudoku, crossword, chess). The view holds
//! no host handle: it answers an input with an [`Act`], and the plugin
//! (`native_plugin.rs`) does what that needs (draws which part again, makes a
//! puzzle, reads a file, asks for the letters, keeps what changed). The
//! drawing is in `native_draw.rs`.
//!
//! Keys: left and right move (round the board square by square, or from one
//! movable piece to the next in chess), up and down (the page keys) move by
//! rows, Select picks the square: a digit for a sudoku, the word to type for
//! a crossword, a piece and then where it goes in chess. Hold Select (or
//! Menu) for the buttons under the board; Back steps out.

use crate::{
    chess::{self, Play},
    native_draw::Geometry,
    puz::{self, Crossword},
    state::Stats,
    sudoku::{self, Game, Level},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::callbacks::{Input, InputGestureKind},
};

/// What is on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Menu,
    Levels,
    Sudoku,
    Files,
    Crossword,
    Clues,
    Chess,
    Stats,
}

/// What to draw again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redraw {
    /// These parts, as partial refreshes.
    Areas(Vec<Rect>),
    /// The whole screen, partially.
    Part,
    /// The whole screen with a full refresh (a new puzzle).
    Full,
}

/// What the plugin has to do after an input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Not for the view.
    Ignored,
    /// Back out of the screen.
    Back,
    Draw(Redraw),
    /// A puzzle to make: of the day, or a free one.
    Generate {
        daily: bool,
        level: Level,
    },
    /// The crossword files of the folder are to be listed.
    ListFiles,
    /// The crossword file `index` of the list is to be read.
    OpenFile(usize),
    /// The letters of the crossword word are to be asked for.
    Type,
}

/// What changed and is to be kept.
pub mod dirty {
    pub const SUDOKU: u8 = 1;
    pub const DAILY: u8 = 2;
    pub const CROSSWORD: u8 = 4;
    pub const STATS: u8 = 8;
}

/// A line of the status row, as a catalog key and its numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    /// The usual line of the board.
    #[default]
    Plain,
    Wrong(usize),
    NoMistakes,
    Solved,
    NotThat,
    Hint,
    Notes,
}

/// Which part of a board has the focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Board,
    /// The button `n` of the row under the board.
    Bar(usize),
}

/// A key as the screens use it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Left,
    Right,
    Up,
    Down,
    Select,
    Alt,
    Back,
}

fn key_of(action: &str) -> Option<Key> {
    Some(match action {
        "core.left" | "core.previous" => Key::Left,
        "core.right" | "core.next" => Key::Right,
        "core.up" | "core.prev-page" | "core.page-prev" => Key::Up,
        "core.down" | "core.next-page" | "core.page-next" => Key::Down,
        "core.confirm" => Key::Select,
        "core.long-confirm" | "core.menu" => Key::Alt,
        "core.back" => Key::Back,
        _ => return None,
    })
}

/// A crossword being played.
#[derive(Debug)]
pub struct CrossGame {
    pub cw: Crossword,
    /// The file's name.
    pub name: String,
    pub cursor: usize,
    pub across: bool,
    /// Whether the wrong letters are marked.
    pub marked: bool,
    /// Whether the solving was counted already.
    pub counted: bool,
}

/// A chess puzzle being solved.
#[derive(Debug)]
pub struct ChessGame {
    pub play: Play,
    /// The board is turned: black is the player's side.
    pub flip: bool,
    pub cursor: u8,
    pub selected: Option<u8>,
    pub targets: Vec<u8>,
    pub hint: Option<chess::Move>,
}

/// The screen.
#[derive(Debug)]
pub struct ScreenView {
    pub mode: Mode,
    pub zone: Zone,
    /// The row of a list that has the focus, and the first row shown.
    pub focus: usize,
    pub first: usize,
    /// The texts of the rows of the list, as the last drawing made them.
    pub rows: Vec<(String, String)>,
    pub rows_stale: bool,
    pub free: Option<Game>,
    pub daily: Option<Game>,
    /// Which sudoku is on the board.
    pub daily_open: bool,
    pub cursor: usize,
    pub notes_on: bool,
    pub palette: bool,
    pub status: Status,
    pub files: Vec<String>,
    pub files_note: bool,
    pub cross: Option<CrossGame>,
    pub chess: Option<ChessGame>,
    /// What changed since the plugin last kept it.
    pub dirty: u8,
    /// The clue list's focus and first row.
    pub geo: Geometry,
    /// The error of a file that was not read, for the status line.
    pub fault: Option<puz::Fault>,
    /// The day today is (days since 1970), where the time is known.
    pub today: Option<i32>,
}

impl Default for ScreenView {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenView {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mode: Mode::Menu,
            zone: Zone::Board,
            focus: 0,
            first: 0,
            rows: Vec::new(),
            rows_stale: true,
            free: None,
            daily: None,
            daily_open: false,
            cursor: 40,
            notes_on: false,
            palette: false,
            status: Status::Plain,
            files: Vec::new(),
            files_note: false,
            cross: None,
            chess: None,
            dirty: 0,
            geo: Geometry::default(),
            fault: None,
            today: None,
        }
    }

    /// Goes to `mode` with the focus on its first row.
    fn go(&mut self, mode: Mode) -> Act {
        self.mode = mode;
        self.zone = Zone::Board;
        self.palette = false;
        self.focus = 0;
        self.first = 0;
        self.rows_stale = true;
        self.status = Status::Plain;
        Act::Draw(Redraw::Part)
    }

    fn game(&self) -> Option<&Game> {
        if self.daily_open {
            self.daily.as_ref()
        } else {
            self.free.as_ref()
        }
    }

    fn game_mut(&mut self) -> Option<&mut Game> {
        if self.daily_open {
            self.daily.as_mut()
        } else {
            self.free.as_mut()
        }
    }

    /// The number of rows the current list has.
    #[must_use]
    pub fn list_len(&self) -> usize {
        match self.mode {
            Mode::Menu => 5,
            Mode::Levels => Level::ALL.len(),
            Mode::Files => self.files.len(),
            Mode::Clues => self
                .cross
                .as_ref()
                .map_or(0, |c| c.cw.across.len() + c.cw.down.len()),
            _ => 0,
        }
    }

    /// Opens the puzzle of the day (a new one if there is none of today).
    pub fn start_daily(&mut self, stats: &Stats) -> Act {
        self.menu_choose(0, stats)
    }

    /// Puts a puzzle in the free slot or the day's, and shows it.
    pub fn start_sudoku(&mut self, game: Game, daily: bool) -> Act {
        self.daily_open = daily;
        if daily {
            self.daily = Some(game);
            self.dirty |= dirty::DAILY;
            self.sync_daily();
        } else {
            self.free = Some(game);
            self.dirty |= dirty::SUDOKU;
        }
        self.cursor = 40;
        self.go(Mode::Sudoku);
        Act::Draw(Redraw::Full)
    }

    /// The puzzle of the day's state, as the widget and the menu show it.
    pub fn sync_daily_to(&self, stats: &mut Stats) {
        if let Some(game) = self.daily.as_ref() {
            stats.daily_day = game.day;
            stats.daily_left = u8::try_from(sudoku::CELLS - game.filled()).unwrap_or(81);
            stats.daily_done = game.solved();
        }
    }

    fn sync_daily(&mut self) {
        self.dirty |= dirty::STATS;
    }

    /// The crossword files, as the folder lists them.
    pub fn set_files(&mut self, files: Vec<String>) -> Act {
        self.files_note = files.is_empty();
        self.files = files;
        self.go(Mode::Files)
    }

    /// A crossword that was read.
    pub fn start_crossword(&mut self, cw: Crossword, name: String) -> Act {
        let counted = cw.solved();
        let cursor = (0..cw.width as usize * cw.height as usize)
            .find(|a| !cw.is_block_at(*a))
            .unwrap_or(0);
        self.cross = Some(CrossGame {
            cw,
            name,
            cursor,
            across: true,
            marked: false,
            counted,
        });
        self.fault = None;
        self.go(Mode::Crossword);
        Act::Draw(Redraw::Full)
    }

    /// A file that could not be read.
    pub fn fail(&mut self, fault: puz::Fault) -> Act {
        self.fault = Some(fault);
        Act::Draw(Redraw::Part)
    }

    /// Puts the chess puzzle `index` on the board.
    pub fn start_chess(&mut self, index: usize) -> Act {
        self.chess = Play::new(index).map(|play| {
            let flip = play.board.black_to_move;
            let mut game = ChessGame {
                play,
                flip,
                cursor: 0,
                selected: None,
                targets: Vec::new(),
                hint: None,
            };
            game.cursor = game.movable().first().copied().unwrap_or(0);
            game
        });
        self.go(Mode::Chess);
        Act::Draw(Redraw::Full)
    }

    /// The letters typed for the word under the cursor.
    pub fn typed(&mut self, text: &str, stats: &mut Stats) -> Act {
        let Some(game) = self.cross.as_mut() else {
            return Act::Ignored;
        };
        let Some(clue) = game.cw.word_at(game.cursor, game.across).cloned() else {
            return Act::Ignored;
        };
        game.cw.fill(&clue, game.across, text);
        game.marked = false;
        let area = word_rect(&self.geo, &game.cw, &clue, game.across);
        self.dirty |= dirty::CROSSWORD;
        self.status = Status::Plain;
        if game.cw.solved() && !game.counted {
            game.counted = true;
            stats.crossword = stats.crossword.saturating_add(1);
            self.dirty |= dirty::STATS;
            self.status = Status::Solved;
        }
        Act::Draw(Redraw::Areas(alloc::vec![
            area,
            self.geo.status,
            self.geo.bar
        ]))
    }

    /// A key or a tap.
    pub fn input(&mut self, input: &Input<'_>, stats: &mut Stats) -> Act {
        match *input {
            Input::Action { action, .. } => match key_of(action) {
                Some(key) => self.key(key, stats),
                None => Act::Ignored,
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => self.tap(position, stats),
            _ => Act::Ignored,
        }
    }

    fn key(&mut self, key: Key, stats: &mut Stats) -> Act {
        match self.mode {
            Mode::Menu | Mode::Levels | Mode::Files | Mode::Clues => self.list_key(key, stats),
            Mode::Stats => match key {
                Key::Back | Key::Select => self.go(Mode::Menu),
                _ => Act::Ignored,
            },
            Mode::Sudoku => self.sudoku_key(key, stats),
            Mode::Crossword => self.crossword_key(key),
            Mode::Chess => self.chess_key(key, stats),
        }
    }

    /// Back out of the mode: to where it came from, or out of the screen.
    pub fn back(&mut self) -> Act {
        match self.mode {
            Mode::Menu => Act::Back,
            Mode::Clues => self.go(Mode::Crossword),
            Mode::Crossword => self.go(Mode::Files),
            Mode::Levels if self.free.is_some() => self.go(Mode::Sudoku),
            _ => self.go(Mode::Menu),
        }
    }

    // Lists.

    fn list_key(&mut self, key: Key, stats: &mut Stats) -> Act {
        let len = self.list_len();
        let focus = self.focus;
        match key {
            Key::Left | Key::Up if len > 0 => {
                self.move_focus(focus.checked_sub(1).unwrap_or(len - 1))
            }
            Key::Right | Key::Down if len > 0 => self.move_focus((focus + 1) % len),
            Key::Select | Key::Alt => self.choose(self.focus, stats),
            // Back out of the menu is the host's own: it leaves the screen.
            Key::Back if self.mode == Mode::Menu => Act::Ignored,
            Key::Back => self.back(),
            _ => Act::Ignored,
        }
    }

    fn move_focus(&mut self, to: usize) -> Act {
        let from = core::mem::replace(&mut self.focus, to);
        let before = self.first;
        let visible = self.geo.visible.max(1);
        if to < self.first {
            self.first = to;
        } else if to >= self.first + visible {
            self.first = to + 1 - visible;
        }
        if self.first != before {
            return Act::Draw(Redraw::Part);
        }
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.row_rect(from.saturating_sub(self.first)),
            self.geo.row_rect(to.saturating_sub(self.first)),
        ]))
    }

    fn choose(&mut self, row: usize, stats: &Stats) -> Act {
        match self.mode {
            Mode::Menu => self.menu_choose(row, stats),
            Mode::Levels => {
                let level = Level::from_index(u8::try_from(row).unwrap_or(1));
                Act::Generate {
                    daily: false,
                    level,
                }
            }
            Mode::Files if row < self.files.len() => Act::OpenFile(row),
            Mode::Clues => self.clue_choose(row),
            _ => Act::Ignored,
        }
    }

    fn menu_choose(&mut self, row: usize, stats: &Stats) -> Act {
        match row {
            0 => {
                if self
                    .daily
                    .as_ref()
                    .is_some_and(|g| Some(g.day) == self.today)
                {
                    self.daily_open = true;
                    self.cursor = 40;
                    self.go(Mode::Sudoku)
                } else {
                    Act::Generate {
                        daily: true,
                        level: Level::Medium,
                    }
                }
            }
            1 => {
                self.daily_open = false;
                if self.free.is_some() {
                    self.go(Mode::Sudoku)
                } else {
                    self.go(Mode::Levels)
                }
            }
            2 => Act::ListFiles,
            3 => {
                let next = usize::try_from(stats.chess_next).unwrap_or(0);
                self.start_chess(next)
            }
            _ => self.go(Mode::Stats),
        }
    }

    fn clue_choose(&mut self, row: usize) -> Act {
        let Some(game) = self.cross.as_mut() else {
            return Act::Ignored;
        };
        let across = row < game.cw.across.len();
        let clue = if across {
            game.cw.across.get(row)
        } else {
            game.cw.down.get(row - game.cw.across.len())
        };
        if let Some(clue) = clue {
            game.cursor = usize::from(clue.cell);
            game.across = across;
        }
        self.go(Mode::Crossword)
    }

    // Taps.

    fn tap(&mut self, at: Point, stats: &mut Stats) -> Act {
        if self.geo.back.is_some_and(|r| contains(r, at)) {
            return self.back();
        }
        match self.mode {
            Mode::Menu | Mode::Levels | Mode::Files | Mode::Clues => {
                match self.geo.row_at(at, self.first, self.list_len()) {
                    Some(row) => {
                        self.focus = row;
                        self.choose(row, stats)
                    }
                    None => Act::Ignored,
                }
            }
            Mode::Stats => Act::Ignored,
            Mode::Sudoku | Mode::Crossword | Mode::Chess => {
                if let Some(i) = self.geo.bar_at(at) {
                    self.zone = Zone::Bar(i);
                    return self.press(i, stats);
                }
                match self.geo.cell_at(at) {
                    Some(cell) => self.tap_cell(cell, stats),
                    None => Act::Ignored,
                }
            }
        }
    }

    fn tap_cell(&mut self, cell: usize, stats: &mut Stats) -> Act {
        self.zone = Zone::Board;
        match self.mode {
            Mode::Sudoku => {
                let same = self.cursor == cell;
                let moved = self.set_cursor(cell);
                if same {
                    self.key(Key::Select, stats)
                } else {
                    moved
                }
            }
            Mode::Crossword => {
                let same = self.cross.as_ref().is_some_and(|g| g.cursor == cell);
                if same {
                    self.crossword_key(Key::Select)
                } else {
                    self.cross_goto(cell, None)
                }
            }
            _ => self.chess_tap(cell, stats),
        }
    }

    /// Whichever button `i` of the row under the board is.
    fn press(&mut self, i: usize, stats: &mut Stats) -> Act {
        match self.mode {
            Mode::Sudoku => self.sudoku_press(i, stats),
            Mode::Crossword => self.crossword_press(i),
            _ => self.chess_press(i, stats),
        }
    }

    // Sudoku.

    fn set_cursor(&mut self, to: usize) -> Act {
        let from = core::mem::replace(&mut self.cursor, to.min(sudoku::CELLS - 1));
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.cell_rect(from).grow(4),
            self.geo.cell_rect(self.cursor).grow(4),
            self.geo.bar,
        ]))
    }

    fn sudoku_key(&mut self, key: Key, stats: &mut Stats) -> Act {
        let cell = self.cursor;
        match (self.zone, key) {
            (_, Key::Back) => self.sudoku_back(),
            (Zone::Board, Key::Left) => self.set_cursor((cell + sudoku::CELLS - 1) % sudoku::CELLS),
            (Zone::Board, Key::Right) => self.set_cursor((cell + 1) % sudoku::CELLS),
            (Zone::Board, Key::Up) => self.set_cursor((cell + sudoku::CELLS - 9) % sudoku::CELLS),
            (Zone::Board, Key::Down) if cell >= 72 => self.enter_bar(0),
            (Zone::Board, Key::Down) => self.set_cursor(cell + 9),
            (Zone::Board, Key::Select) => self.open_palette(),
            (Zone::Board, Key::Alt) => self.enter_bar(0),
            (Zone::Bar(i), Key::Left) => self.enter_bar(i.saturating_sub(1)),
            (Zone::Bar(i), Key::Right) => self.enter_bar((i + 1).min(self.bar_len() - 1)),
            (Zone::Bar(_), Key::Up | Key::Alt) => self.leave_bar(),
            (Zone::Bar(_), Key::Down) => Act::Ignored,
            (Zone::Bar(i), Key::Select) => self.sudoku_press(i, stats),
        }
    }

    fn sudoku_back(&mut self) -> Act {
        if self.palette || matches!(self.zone, Zone::Bar(_)) {
            return self.leave_bar();
        }
        self.go(Mode::Menu)
    }

    fn bar_len(&self) -> usize {
        match self.mode {
            Mode::Sudoku if self.palette => 10,
            Mode::Sudoku if self.daily_open => 3,
            Mode::Sudoku | Mode::Crossword => 4,
            _ => 3,
        }
    }

    fn enter_bar(&mut self, i: usize) -> Act {
        self.zone = Zone::Bar(i.min(self.bar_len() - 1));
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.bar,
            self.geo.cell_rect(self.cursor).grow(4),
        ]))
    }

    fn leave_bar(&mut self) -> Act {
        self.zone = Zone::Board;
        self.palette = false;
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.bar,
            self.geo.cell_rect(self.cursor).grow(4),
            self.geo.status,
        ]))
    }

    fn open_palette(&mut self) -> Act {
        let cell = self.cursor;
        let open = self
            .game()
            .is_some_and(|g| !g.is_given(cell) && !g.solved());
        if !open {
            return Act::Ignored;
        }
        self.palette = true;
        let digit = self.game().map_or(0, |g| usize::from(g.cur[cell]));
        self.zone = Zone::Bar(if digit == 0 { 0 } else { digit - 1 });
        Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar, self.geo.status]))
    }

    fn sudoku_press(&mut self, i: usize, stats: &mut Stats) -> Act {
        if self.palette {
            return self.place(i, stats);
        }
        match i {
            0 => {
                self.notes_on = !self.notes_on;
                self.status = if self.notes_on {
                    Status::Notes
                } else {
                    Status::Plain
                };
                Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar, self.geo.status]))
            }
            1 => self.check(),
            2 => self.undo(),
            _ => self.go(Mode::Levels),
        }
    }

    fn check(&mut self) -> Act {
        let Some(game) = self.game_mut() else {
            return Act::Ignored;
        };
        game.marked = true;
        let wrong = game.wrong_count();
        self.status = if wrong == 0 {
            Status::NoMistakes
        } else {
            Status::Wrong(wrong)
        };
        Act::Draw(Redraw::Part)
    }

    fn undo(&mut self) -> Act {
        let was_marked = self.game().is_some_and(|g| g.marked);
        let Some(cell) = self.game_mut().and_then(Game::undo) else {
            return Act::Ignored;
        };
        self.after_change(was_marked, cell)
    }

    /// What a change to `cell` redraws: it, the status, and the whole board
    /// where the wrong marks were on.
    fn after_change(&mut self, was_marked: bool, cell: usize) -> Act {
        self.dirty |= if self.daily_open {
            dirty::DAILY
        } else {
            dirty::SUDOKU
        };
        self.status = Status::Plain;
        if was_marked {
            return Act::Draw(Redraw::Part);
        }
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.cell_rect(cell).grow(4),
            self.geo.status,
            self.geo.bar,
        ]))
    }

    /// The button `i` of the digits: 0..9 a digit (a pencil mark where
    /// notes are on), 9 clears.
    fn place(&mut self, i: usize, stats: &mut Stats) -> Act {
        let (cell, notes) = (self.cursor, self.notes_on);
        let digit = u8::try_from(if i >= 9 { 0 } else { i + 1 }).unwrap_or(0);
        let was_marked = self.game().is_some_and(|g| g.marked);
        let Some(game) = self.game_mut() else {
            return Act::Ignored;
        };
        let done = if notes && digit != 0 {
            game.note(cell, digit)
        } else {
            game.set(cell, digit)
        };
        let solved = game.solved();
        self.zone = Zone::Board;
        self.palette = false;
        if !done {
            return Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar, self.geo.status]));
        }
        let act = self.after_change(was_marked, cell);
        if solved {
            self.status = Status::Solved;
            if self.daily_open {
                stats.sudoku = stats.sudoku.saturating_add(1);
                self.sync_daily();
            } else {
                stats.sudoku = stats.sudoku.saturating_add(1);
            }
            self.dirty |= dirty::STATS;
            return Act::Draw(Redraw::Part);
        }
        act
    }

    // Crossword.

    fn crossword_key(&mut self, key: Key) -> Act {
        let Some(game) = self.cross.as_ref() else {
            return Act::Ignored;
        };
        let at = game.cursor;
        let last_row = at / usize::from(game.cw.width) + 1 >= usize::from(game.cw.height);
        match (self.zone, key) {
            (Zone::Board, Key::Left) => self.cross_move(at, -1, 0),
            (Zone::Board, Key::Right) => self.cross_move(at, 1, 0),
            (Zone::Board, Key::Up) => self.cross_move(at, 0, -1),
            (Zone::Board, Key::Down) if last_row => self.enter_bar_cw(0),
            (Zone::Board, Key::Down) => self.cross_move(at, 0, 1),
            (Zone::Board, Key::Select) => Act::Type,
            (Zone::Board, Key::Alt) => self.enter_bar_cw(0),
            (Zone::Board, Key::Back) => self.back(),
            (Zone::Bar(i), Key::Left) => self.enter_bar_cw(i.saturating_sub(1)),
            (Zone::Bar(i), Key::Right) => self.enter_bar_cw((i + 1).min(3)),
            (Zone::Bar(_), Key::Up | Key::Alt | Key::Back) => self.leave_bar_cw(),
            (Zone::Bar(_), Key::Down) => Act::Ignored,
            (Zone::Bar(i), Key::Select) => self.crossword_press(i),
        }
    }

    fn enter_bar_cw(&mut self, i: usize) -> Act {
        self.zone = Zone::Bar(i);
        Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar]))
    }

    fn leave_bar_cw(&mut self) -> Act {
        self.zone = Zone::Board;
        Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar]))
    }

    /// Moves the cursor one step, past black squares; the direction follows.
    fn cross_move(&mut self, at: usize, dx: isize, dy: isize) -> Act {
        let Some(game) = self.cross.as_ref() else {
            return Act::Ignored;
        };
        let (w, h) = (isize::from(game.cw.width), isize::from(game.cw.height));
        let (mut x, mut y) = (
            isize::try_from(at % w as usize).unwrap_or(0),
            isize::try_from(at / w as usize).unwrap_or(0),
        );
        loop {
            x += dx;
            y += dy;
            if x < 0 || y < 0 || x >= w || y >= h {
                return Act::Ignored;
            }
            let to = usize::try_from(y * w + x).unwrap_or(0);
            if !game.cw.is_block_at(to) {
                return self.cross_goto(to, Some(dx != 0));
            }
        }
    }

    /// Puts the cursor on `to`; `across` sets the direction.
    fn cross_goto(&mut self, to: usize, across: Option<bool>) -> Act {
        let Some(game) = self.cross.as_mut() else {
            return Act::Ignored;
        };
        if game.cw.is_block_at(to) {
            return Act::Ignored;
        }
        let before = (game.cursor, game.across);
        game.cursor = to;
        if let Some(across) = across {
            // A direction with no word through the square is not taken.
            if game.cw.word_at(to, across).is_some() {
                game.across = across;
            }
        }
        if game.cw.word_at(to, game.across).is_none() {
            game.across = !game.across;
        }
        let old_word = game.cw.word_at(before.0, before.1).cloned();
        let new_word = game.cw.word_at(game.cursor, game.across).cloned();
        let (cursor, across) = (game.cursor, game.across);
        let mut areas = alloc::vec![
            self.geo.cell_rect(before.0).grow(4),
            self.geo.cell_rect(cursor).grow(4),
            self.geo.status,
        ];
        if (old_word != new_word || before.1 != across)
            && let (Some(old), Some(new)) = (old_word, new_word)
        {
            areas.push(word_rect(&self.geo, &game.cw, &old, before.1));
            areas.push(word_rect(&self.geo, &game.cw, &new, across));
        }
        Act::Draw(Redraw::Areas(areas))
    }

    fn crossword_press(&mut self, i: usize) -> Act {
        match i {
            0 => {
                let Some(game) = self.cross.as_mut() else {
                    return Act::Ignored;
                };
                let flipped = !game.across;
                if game.cw.word_at(game.cursor, flipped).is_some() {
                    game.across = flipped;
                }
                Act::Draw(Redraw::Part)
            }
            1 => self.go(Mode::Clues),
            2 => self.cross_check(),
            _ => self.cross_reveal(),
        }
    }

    fn cross_check(&mut self) -> Act {
        let Some(game) = self.cross.as_mut() else {
            return Act::Ignored;
        };
        game.marked = true;
        let wrong = game.cw.wrong_count();
        self.status = if wrong == 0 {
            Status::NoMistakes
        } else {
            Status::Wrong(wrong)
        };
        Act::Draw(Redraw::Part)
    }

    fn cross_reveal(&mut self) -> Act {
        let Some(game) = self.cross.as_mut() else {
            return Act::Ignored;
        };
        let Some(clue) = game.cw.word_at(game.cursor, game.across).cloned() else {
            return Act::Ignored;
        };
        game.cw.reveal(&clue, game.across);
        game.marked = false;
        self.dirty |= dirty::CROSSWORD;
        self.status = Status::Plain;
        Act::Draw(Redraw::Part)
    }

    // Chess.

    fn chess_key(&mut self, key: Key, stats: &mut Stats) -> Act {
        let Some(game) = self.chess.as_ref() else {
            return Act::Ignored;
        };
        let (cursor, solved) = (game.cursor, game.play.solved);
        match (self.zone, key) {
            (_, Key::Back) => self.chess_back(),
            (Zone::Board, Key::Left | Key::Up) => self.chess_cycle(false),
            (Zone::Board, Key::Right | Key::Down) => self.chess_cycle(true),
            (Zone::Board, Key::Select) => self.chess_tap_square(cursor, stats),
            (Zone::Board, Key::Alt) => self.enter_bar_chess(usize::from(solved)),
            (Zone::Bar(i), Key::Left | Key::Up) => self.enter_bar_chess(i.saturating_sub(1)),
            (Zone::Bar(i), Key::Right | Key::Down) => self.enter_bar_chess((i + 1).min(2)),
            (Zone::Bar(_), Key::Alt) => self.chess_leave_bar(),
            (Zone::Bar(i), Key::Select) => self.chess_press(i, stats),
        }
    }

    fn chess_back(&mut self) -> Act {
        if matches!(self.zone, Zone::Bar(_)) {
            return self.chess_leave_bar();
        }
        if let Some(game) = self.chess.as_mut()
            && game.selected.take().is_some()
        {
            game.targets.clear();
            return Act::Draw(Redraw::Part);
        }
        self.go(Mode::Menu)
    }

    fn enter_bar_chess(&mut self, i: usize) -> Act {
        self.zone = Zone::Bar(i);
        Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar]))
    }

    fn chess_leave_bar(&mut self) -> Act {
        self.zone = Zone::Board;
        Act::Draw(Redraw::Areas(alloc::vec![self.geo.bar]))
    }

    /// Cursor moves among the squares that matter now: the targets of the
    /// selected piece, else the pieces that can move.
    fn chess_cycle(&mut self, forward: bool) -> Act {
        let Some(game) = self.chess.as_mut() else {
            return Act::Ignored;
        };
        let list = if game.selected.is_some() {
            game.targets.clone()
        } else {
            game.movable()
        };
        if list.is_empty() {
            return Act::Ignored;
        }
        let mut sorted = list;
        paperpaw_sdk::sort::sort_by_key(&mut sorted, |s| game.display(*s));
        let at = sorted.iter().position(|s| *s == game.cursor);
        let to = match (at, forward) {
            (Some(i), true) => sorted[(i + 1) % sorted.len()],
            (Some(i), false) => sorted[(i + sorted.len() - 1) % sorted.len()],
            (None, _) => sorted[0],
        };
        let from = core::mem::replace(&mut game.cursor, to);
        Act::Draw(Redraw::Areas(alloc::vec![
            self.geo.cell_rect(game.display(from)).grow(4),
            self.geo.cell_rect(game.display(to)).grow(4),
        ]))
    }

    fn chess_tap(&mut self, display: usize, stats: &mut Stats) -> Act {
        let Some(game) = self.chess.as_ref() else {
            return Act::Ignored;
        };
        let square = game.square(display);
        self.chess_tap_square(square, stats)
    }

    /// Selects a piece, moves it, or lets go of it.
    fn chess_tap_square(&mut self, square: u8, stats: &mut Stats) -> Act {
        let Some(game) = self.chess.as_mut() else {
            return Act::Ignored;
        };
        if game.play.solved {
            return Act::Ignored;
        }
        game.hint = None;
        game.cursor = square;
        self.status = Status::Plain;
        if let Some(from) = game.selected
            && game.targets.contains(&square)
        {
            return self.chess_move(from, square, stats);
        }
        let piece = game.play.board.at(square);
        if piece != 0 && chess::is_black(piece) == game.play.board.black_to_move {
            let moves = game.play.board.moves_from(square);
            if !moves.is_empty() {
                game.selected = Some(square);
                let mut targets: Vec<u8> = moves.iter().map(|m| m.to).collect();
                targets.dedup();
                game.targets = targets;
                game.cursor = game.targets.first().copied().unwrap_or(square);
                return Act::Draw(Redraw::Part);
            }
        }
        game.selected = None;
        game.targets.clear();
        Act::Draw(Redraw::Part)
    }

    fn chess_move(&mut self, from: u8, to: u8, stats: &mut Stats) -> Act {
        let Some(game) = self.chess.as_mut() else {
            return Act::Ignored;
        };
        game.selected = None;
        game.targets.clear();
        let outcome = game.play.play(from, to, 0);
        game.cursor = game.movable().first().copied().unwrap_or(to);
        self.status = match outcome {
            chess::Outcome::Wrong | chess::Outcome::Illegal => Status::NotThat,
            chess::Outcome::Solved => Status::Solved,
            chess::Outcome::Right => Status::Plain,
        };
        if outcome == chess::Outcome::Solved {
            stats.chess = stats.chess.saturating_add(1);
            let total = chess::count().max(1);
            stats.chess_next = u32::try_from((game.play.index + 1) % total).unwrap_or(0);
            self.dirty |= dirty::STATS;
            self.zone = Zone::Bar(1);
        }
        Act::Draw(Redraw::Part)
    }

    /// The bar of a chess puzzle: Hint, Next, Restart.
    fn chess_press(&mut self, i: usize, stats: &mut Stats) -> Act {
        let Some(game) = self.chess.as_mut() else {
            return Act::Ignored;
        };
        let index = game.play.index;
        match i {
            0 => {
                game.hint = game.play.hint();
                game.selected = None;
                game.targets.clear();
                self.status = Status::Hint;
                Act::Draw(Redraw::Part)
            }
            1 => {
                let total = chess::count().max(1);
                let next = (index + 1) % total;
                stats.chess_next = u32::try_from(next).unwrap_or(0);
                self.dirty |= dirty::STATS;
                self.start_chess(next)
            }
            _ => self.start_chess(index),
        }
    }
}

impl ChessGame {
    /// The square shown at display index `d` (row-major from the top left).
    #[must_use]
    pub fn square(&self, d: usize) -> u8 {
        let d = u8::try_from(d.min(63)).unwrap_or(0);
        if self.flip { 63 - d } else { d }
    }

    /// Where `square` is shown.
    #[must_use]
    pub fn display(&self, square: u8) -> usize {
        usize::from(if self.flip { 63 - square } else { square })
    }

    /// The squares of the pieces that can move.
    #[must_use]
    pub fn movable(&self) -> Vec<u8> {
        let mut from: Vec<u8> = self
            .play
            .board
            .legal_moves()
            .iter()
            .map(|m| m.from)
            .collect();
        from.sort_unstable();
        from.dedup();
        from
    }
}

fn contains(r: Rect, p: Point) -> bool {
    p.x >= i32::from(r.x)
        && p.y >= i32::from(r.y)
        && p.x < i32::from(r.x) + i32::from(r.width)
        && p.y < i32::from(r.y) + i32::from(r.height)
}

/// The rectangle a word covers on the grid.
fn word_rect(geo: &Geometry, cw: &Crossword, clue: &puz::Clue, across: bool) -> Rect {
    let mut cells = cw.cells_of(clue, across);
    let first = cells
        .next()
        .map_or(Rect::new(0, 0, 0, 0), |c| geo.cell_rect(c));
    let last = cells.last().map_or(first, |c| geo.cell_rect(c));
    first.union(&last).grow(4)
}
