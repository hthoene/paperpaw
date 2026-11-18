//! Sudoku with no host: a solver that counts solutions, a generator whose
//! puzzles have exactly one, and the game a reader plays (values, pencil
//! marks, undo, check) with its kept text. Nothing here allocates beyond the
//! kept text, and nothing recurses: the solver keeps its own stack.

use alloc::{string::String, vec::Vec};

/// The cells of a grid.
pub const CELLS: usize = 81;
/// The most moves undo remembers.
pub const UNDO: usize = 48;
/// How many search steps the generator spends on judging one removal; a
/// removal it cannot judge in that is not made.
const BUDGET: u32 = 4_000;

/// How hard a puzzle is: by the number of clues left (and so by how far the
/// player must look ahead).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Easy,
    Medium,
    Hard,
}

impl Level {
    pub const ALL: [Level; 3] = [Level::Easy, Level::Medium, Level::Hard];

    /// The clues the generator tries to leave.
    #[must_use]
    pub const fn clues(self) -> usize {
        match self {
            Level::Easy => 40,
            Level::Medium => 32,
            Level::Hard => 26,
        }
    }

    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            Level::Easy => 0,
            Level::Medium => 1,
            Level::Hard => 2,
        }
    }

    #[must_use]
    pub const fn from_index(index: u8) -> Self {
        match index {
            0 => Level::Easy,
            1 => Level::Medium,
            _ => Level::Hard,
        }
    }
}

/// A small, fast generator of numbers (xorshift32); the same seed gives the
/// same puzzle, which is how the puzzle of the day is the same for a day.
#[derive(Debug, Clone, Copy)]
pub struct Rng(u32);

impl Rng {
    #[must_use]
    pub fn new(seed: u32) -> Self {
        // Mixed so that neighbouring seeds (days) give different streams.
        let mixed = seed.wrapping_mul(0x9E37_79B1) ^ 0x85EB_CA6B;
        Self(if mixed == 0 { 0x1234_5678 } else { mixed })
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A number below `n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u32() >> 8) as usize % n.max(1)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i + 1);
            items.swap(i, j);
        }
    }
}

const fn row(cell: usize) -> usize {
    cell / 9
}
const fn col(cell: usize) -> usize {
    cell % 9
}
const fn boxed(cell: usize) -> usize {
    row(cell) / 3 * 3 + col(cell) / 3
}

/// A search state: the digits placed and what each row, column and box has.
struct Solver {
    cells: [u8; CELLS],
    rows: [u16; 9],
    cols: [u16; 9],
    boxes: [u16; 9],
    first: Option<[u8; CELLS]>,
}

impl Solver {
    fn new(grid: &[u8; CELLS]) -> Option<Self> {
        let mut s = Self {
            cells: [0; CELLS],
            rows: [0; 9],
            cols: [0; 9],
            boxes: [0; 9],
            first: None,
        };
        for (cell, &digit) in grid.iter().enumerate() {
            if digit == 0 {
                continue;
            }
            if !(1..=9).contains(&digit) || s.mask(cell) & (1 << (digit - 1)) == 0 {
                return None;
            }
            s.place(cell, digit);
        }
        Some(s)
    }

    /// The digits that fit `cell`, one bit each.
    fn mask(&self, cell: usize) -> u16 {
        !(self.rows[row(cell)] | self.cols[col(cell)] | self.boxes[boxed(cell)]) & 0x1FF
    }

    fn place(&mut self, cell: usize, digit: u8) {
        let bit = 1u16 << (digit - 1);
        self.cells[cell] = digit;
        self.rows[row(cell)] |= bit;
        self.cols[col(cell)] |= bit;
        self.boxes[boxed(cell)] |= bit;
    }

    fn unplace(&mut self, cell: usize) {
        let bit = 1u16 << (self.cells[cell] - 1);
        self.cells[cell] = 0;
        self.rows[row(cell)] &= !bit;
        self.cols[col(cell)] &= !bit;
        self.boxes[boxed(cell)] &= !bit;
    }

    /// The empty cell with the fewest digits that fit, and those digits;
    /// `None` when the grid is full.
    fn pick(&self) -> Option<(usize, u16)> {
        let mut best: Option<(usize, u16)> = None;
        for cell in 0..CELLS {
            if self.cells[cell] != 0 {
                continue;
            }
            let mask = self.mask(cell);
            let n = mask.count_ones();
            if best.is_none_or(|(_, m)| n < m.count_ones()) {
                best = Some((cell, mask));
                if n <= 1 {
                    break;
                }
            }
        }
        best
    }

    /// How many solutions there are, up to `limit`; `budget` counts the
    /// steps left, and a search that runs out of them stops.
    fn count(&mut self, limit: u32, budget: &mut u32) -> u32 {
        let mut stack = [(0u8, 0u16); CELLS];
        let mut depth = 0usize;
        let mut found = 0;
        loop {
            if *budget == 0 {
                return found;
            }
            *budget -= 1;
            match self.pick() {
                Some((cell, mask)) if mask != 0 => {
                    if let Some(slot) = stack.get_mut(depth) {
                        *slot = (u8::try_from(cell).unwrap_or(0), mask);
                        depth += 1;
                    }
                }
                Some(_) => {}
                None => {
                    found += 1;
                    if self.first.is_none() {
                        self.first = Some(self.cells);
                    }
                    if found >= limit {
                        return found;
                    }
                }
            }
            // Take the next digit of the deepest cell that has one left.
            loop {
                let Some((cell, mask)) = depth.checked_sub(1).and_then(|d| stack.get(d).copied())
                else {
                    return found;
                };
                let cell = usize::from(cell);
                if self.cells[cell] != 0 {
                    self.unplace(cell);
                }
                if mask == 0 {
                    depth -= 1;
                    continue;
                }
                let bit = mask & mask.wrapping_neg();
                if let Some(slot) = stack.get_mut(depth - 1) {
                    slot.1 = mask & !bit;
                }
                self.place(cell, u8::try_from(bit.trailing_zeros() + 1).unwrap_or(1));
                break;
            }
        }
    }
}

/// The solution of `grid` if it has exactly one.
#[must_use]
pub fn solve_unique(grid: &[u8; CELLS]) -> Option<[u8; CELLS]> {
    let mut solver = Solver::new(grid)?;
    let mut budget = u32::MAX;
    (solver.count(2, &mut budget) == 1)
        .then_some(solver.first)
        .flatten()
}

/// How many solutions `grid` has, up to two.
#[must_use]
pub fn solutions(grid: &[u8; CELLS]) -> u32 {
    let Some(mut solver) = Solver::new(grid) else {
        return 0;
    };
    let mut budget = u32::MAX;
    solver.count(2, &mut budget)
}

/// A full grid made by shuffling the standard pattern: digits, rows within
/// their bands, bands, columns within their stacks, stacks, and a turn.
fn full_grid(rng: &mut Rng) -> [u8; CELLS] {
    let mut digits = [1u8, 2, 3, 4, 5, 6, 7, 8, 9];
    rng.shuffle(&mut digits);
    let mut order = [[0usize, 1, 2], [0, 1, 2], [0, 1, 2], [0, 1, 2]];
    for part in &mut order {
        rng.shuffle(part);
    }
    let mut bands = [0usize, 1, 2];
    let mut stacks = [0usize, 1, 2];
    rng.shuffle(&mut bands);
    rng.shuffle(&mut stacks);
    let turn = rng.below(2) == 1;
    let mut grid = [0u8; CELLS];
    for r in 0..9 {
        for c in 0..9 {
            let rr = bands[r / 3] * 3 + order[0][r % 3];
            let cc = stacks[c / 3] * 3 + order[1][c % 3];
            let pattern = (rr % 3 * 3 + rr / 3 + cc) % 9;
            let (a, b) = if turn { (c, r) } else { (r, c) };
            grid[a * 9 + b] = digits[pattern];
        }
    }
    grid
}

/// A puzzle of `level` with exactly one solution, and that solution. The
/// same seed gives the same pair.
#[must_use]
pub fn generate(seed: u32, level: Level) -> ([u8; CELLS], [u8; CELLS]) {
    let mut rng = Rng::new(seed);
    let solution = full_grid(&mut rng);
    let mut puzzle = solution;
    let mut order = [0u8; CELLS];
    for (i, slot) in order.iter_mut().enumerate() {
        *slot = u8::try_from(i).unwrap_or(0);
    }
    rng.shuffle(&mut order);
    let mut clues = CELLS;
    for &cell in &order {
        if clues <= level.clues() {
            break;
        }
        let cell = usize::from(cell);
        let kept = puzzle[cell];
        puzzle[cell] = 0;
        let Some(mut solver) = Solver::new(&puzzle) else {
            puzzle[cell] = kept;
            continue;
        };
        let mut budget = BUDGET;
        // Only a removal the search proves leaves one solution is made.
        if solver.count(2, &mut budget) == 1 && budget > 0 {
            clues -= 1;
        } else {
            puzzle[cell] = kept;
        }
    }
    (puzzle, solution)
}

/// One move undo can take back: the cell, its value and its pencil marks
/// before.
#[derive(Debug, Clone, Copy, Default)]
struct Move {
    cell: u8,
    value: u8,
    notes: u16,
}

/// The game being played.
#[derive(Debug, Clone)]
pub struct Game {
    pub given: [u8; CELLS],
    pub cur: [u8; CELLS],
    pub solution: [u8; CELLS],
    pub notes: [u16; CELLS],
    pub level: Level,
    pub seed: u32,
    /// The day of the puzzle of the day (days since 1970), 0 for a free game.
    pub day: i32,
    /// Whether the wrong values are marked (until the next change).
    pub marked: bool,
    undo: [Move; UNDO],
    undone: usize,
}

impl Game {
    #[must_use]
    pub fn new(seed: u32, level: Level, day: i32) -> Self {
        let (given, solution) = generate(seed, level);
        Self {
            given,
            cur: given,
            solution,
            notes: [0; CELLS],
            level,
            seed,
            day,
            marked: false,
            undo: [Move::default(); UNDO],
            undone: 0,
        }
    }

    #[must_use]
    pub fn is_given(&self, cell: usize) -> bool {
        self.given.get(cell).is_some_and(|d| *d != 0)
    }

    fn remember(&mut self, cell: usize) {
        if self.undone == UNDO {
            self.undo.copy_within(1.., 0);
            self.undone -= 1;
        }
        self.undo[self.undone] = Move {
            cell: u8::try_from(cell).unwrap_or(0),
            value: self.cur[cell],
            notes: self.notes[cell],
        };
        self.undone += 1;
        self.marked = false;
    }

    /// Puts `digit` (0 clears) in `cell`; false where the cell is a clue.
    pub fn set(&mut self, cell: usize, digit: u8) -> bool {
        if cell >= CELLS || self.is_given(cell) || digit > 9 {
            return false;
        }
        self.remember(cell);
        self.cur[cell] = digit;
        self.notes[cell] = 0;
        true
    }

    /// Turns the pencil mark `digit` on or off in `cell`; false where the
    /// cell is a clue or has a value.
    pub fn note(&mut self, cell: usize, digit: u8) -> bool {
        if cell >= CELLS || self.is_given(cell) || self.cur[cell] != 0 || !(1..=9).contains(&digit)
        {
            return false;
        }
        self.remember(cell);
        self.notes[cell] ^= 1 << (digit - 1);
        true
    }

    /// Takes back the last move; the cell it changed.
    pub fn undo(&mut self) -> Option<usize> {
        self.undone = self.undone.checked_sub(1)?;
        let m = self.undo[self.undone];
        let cell = usize::from(m.cell);
        self.cur[cell] = m.value;
        self.notes[cell] = m.notes;
        self.marked = false;
        Some(cell)
    }

    /// The values that differ from the solution.
    #[must_use]
    pub fn wrong(&self, cell: usize) -> bool {
        self.cur[cell] != 0 && self.cur[cell] != self.solution[cell]
    }

    #[must_use]
    pub fn wrong_count(&self) -> usize {
        (0..CELLS).filter(|c| self.wrong(*c)).count()
    }

    #[must_use]
    pub fn filled(&self) -> usize {
        self.cur.iter().filter(|d| **d != 0).count()
    }

    #[must_use]
    pub fn solved(&self) -> bool {
        self.cur == self.solution
    }

    /// The kept text: `seed|level|day|given|cur|notes`, the digits as
    /// characters (`.` for none), the marks three hex digits a cell.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(32 + CELLS * 5);
        for part in [
            u64::from(self.seed),
            u64::from(self.level.index()),
            u64::try_from(self.day).unwrap_or(0),
        ] {
            push_number(&mut out, part);
            out.push('|');
        }
        for grid in [&self.given, &self.cur] {
            for d in grid {
                out.push(if *d == 0 { '.' } else { char::from(b'0' + d) });
            }
            out.push('|');
        }
        for n in &self.notes {
            for shift in [8, 4, 0] {
                out.push(char::from_digit(u32::from(n >> shift) & 15, 16).unwrap_or('0'));
            }
        }
        out
    }

    /// The game of a kept text; none for one that is damaged or whose
    /// clues do not have exactly one solution.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let mut parts = text.trim().split('|');
        let seed: u32 = parts.next()?.parse().ok()?;
        let level = Level::from_index(parts.next()?.parse().ok()?);
        let day: i32 = parts.next()?.parse().ok()?;
        let given = grid_of(parts.next()?)?;
        let cur = grid_of(parts.next()?)?;
        let notes_text = parts.next()?;
        if parts.next().is_some() || notes_text.len() != CELLS * 3 {
            return None;
        }
        let mut notes = [0u16; CELLS];
        for (i, n) in notes.iter_mut().enumerate() {
            let chunk = notes_text.get(i * 3..i * 3 + 3)?;
            *n = u16::from_str_radix(chunk, 16).ok().filter(|n| *n < 512)?;
        }
        let solution = solve_unique(&given)?;
        // The player's values sit on the clues.
        if (0..CELLS).any(|c| given[c] != 0 && cur[c] != given[c]) {
            return None;
        }
        Some(Self {
            given,
            cur,
            solution,
            notes,
            level,
            seed,
            day,
            marked: false,
            undo: [Move::default(); UNDO],
            undone: 0,
        })
    }
}

fn grid_of(text: &str) -> Option<[u8; CELLS]> {
    if text.len() != CELLS {
        return None;
    }
    let mut grid = [0u8; CELLS];
    for (slot, b) in grid.iter_mut().zip(text.bytes()) {
        *slot = match b {
            b'.' => 0,
            b'1'..=b'9' => b - b'0',
            _ => return None,
        };
    }
    Some(grid)
}

/// Writes `value` in decimal.
pub fn push_number(out: &mut String, mut value: u64) {
    let mut digits = Vec::new();
    loop {
        digits.push(b'0' + u8::try_from(value % 10).unwrap_or(0));
        value /= 10;
        if value == 0 {
            break;
        }
    }
    for d in digits.iter().rev() {
        out.push(char::from(*d));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid(grid: &[u8; CELLS]) -> bool {
        let mut s = Solver::new(&[0; CELLS]).unwrap();
        for (c, d) in grid.iter().enumerate() {
            if *d == 0 || s.mask(c) & (1 << (d - 1)) == 0 {
                return false;
            }
            s.place(c, *d);
        }
        true
    }

    #[test]
    fn generated_puzzles_have_exactly_one_solution() {
        for (seed, level) in [
            (1, Level::Easy),
            (2, Level::Medium),
            (3, Level::Hard),
            (20_500, Level::Medium),
        ] {
            let (puzzle, solution) = generate(seed, level);
            assert!(valid(&solution), "seed {seed}");
            assert_eq!(solutions(&puzzle), 1, "seed {seed}");
            assert_eq!(solve_unique(&puzzle), Some(solution));
            let clues = puzzle.iter().filter(|d| **d != 0).count();
            assert!((17..55).contains(&clues), "{clues} clues");
            for c in 0..CELLS {
                assert!(puzzle[c] == 0 || puzzle[c] == solution[c]);
            }
        }
    }

    #[test]
    fn the_same_seed_gives_the_same_puzzle_and_levels_differ_in_clues() {
        assert_eq!(generate(9, Level::Medium), generate(9, Level::Medium));
        assert_ne!(generate(9, Level::Medium).0, generate(10, Level::Medium).0);
        let count = |level| generate(5, level).0.iter().filter(|d| **d != 0).count();
        assert!(count(Level::Easy) > count(Level::Hard));
    }

    #[test]
    fn the_solver_counts_solutions() {
        let (puzzle, _) = generate(4, Level::Easy);
        let mut open = puzzle;
        // Clearing every clue leaves many solutions.
        open.fill(0);
        assert_eq!(solutions(&open), 2);
        assert_eq!(solutions(&puzzle), 1);
        // Two equal digits in a row have none.
        let mut bad = [0u8; CELLS];
        bad[0] = 5;
        bad[1] = 5;
        assert_eq!(solutions(&bad), 0);
        assert!(solve_unique(&bad).is_none());
        // A known puzzle with one solution.
        let known =
            "530070000600195000098000060800060003400803001700020006060000280000419005000080079";
        let grid = grid_of(&known.replace('0', ".")).unwrap();
        let solved = solve_unique(&grid).unwrap();
        let text: String = solved.iter().map(|d| char::from(b'0' + d)).collect();
        assert_eq!(
            text,
            "534678912672195348198342567859761423426853791713924856961537284287419635345286179"
        );
    }

    #[test]
    fn moves_notes_undo_and_check() {
        let mut game = Game::new(7, Level::Easy, 0);
        let open = (0..CELLS).find(|c| !game.is_given(*c)).unwrap();
        let clue = (0..CELLS).find(|c| game.is_given(*c)).unwrap();
        assert!(!game.set(clue, 3));
        let right = game.solution[open];
        let wrong = right % 9 + 1;
        assert!(game.set(open, wrong));
        assert_eq!(game.wrong_count(), 1);
        assert!(game.wrong(open));
        assert_eq!(game.undo(), Some(open));
        assert_eq!(game.cur[open], 0);
        assert!(game.note(open, 4));
        assert!(game.note(open, 6));
        assert!(game.note(open, 4));
        assert_eq!(game.notes[open], 1 << 5);
        assert!(game.set(open, right));
        assert_eq!(game.notes[open], 0, "a value clears the marks");
        assert!(!game.note(open, 2), "no marks under a value");
        assert_eq!(game.undo(), Some(open));
        assert_eq!(game.notes[open], 1 << 5, "undo brings the marks back");
        while game.undo().is_some() {}
        assert_eq!(game.undo(), None);
        assert_eq!(game.cur[open], 0);
        let mut won = game.clone();
        won.cur = won.solution;
        assert!(won.solved());
        assert!(!game.solved());
    }

    #[test]
    fn the_kept_text_round_trips_and_a_damaged_one_is_none() {
        let mut game = Game::new(11, Level::Medium, 20_400);
        let open = (0..CELLS).find(|c| !game.is_given(*c)).unwrap();
        game.set(open, game.solution[open]);
        let other = (open + 1..CELLS).find(|c| !game.is_given(*c)).unwrap();
        game.note(other, 9);
        let text = game.text();
        let back = Game::parse(&text).unwrap();
        assert_eq!(back.cur, game.cur);
        assert_eq!(back.given, game.given);
        assert_eq!(back.notes, game.notes);
        assert_eq!(back.solution, game.solution);
        assert_eq!(
            (back.seed, back.day, back.level),
            (11, 20_400, Level::Medium)
        );
        assert!(Game::parse("").is_none());
        assert!(Game::parse(&text[..text.len() - 3]).is_none());
        assert!(Game::parse(&text.replacen('|', "|x", 1)).is_none());
    }

    #[test]
    fn undo_keeps_the_latest_moves_when_full() {
        let mut game = Game::new(3, Level::Easy, 0);
        let open: Vec<usize> = (0..CELLS).filter(|c| !game.is_given(*c)).collect();
        for _ in 0..UNDO + 5 {
            game.set(open[0], 1);
            game.set(open[1], 2);
        }
        let mut n = 0;
        while game.undo().is_some() {
            n += 1;
        }
        assert_eq!(n, UNDO);
    }
}
