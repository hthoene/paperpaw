//! Chess puzzles with no host: a position from FEN, the legal moves (check,
//! castling, en passant, promotion), and the bundled puzzles (Lichess, CC0:
//! the position before the opponent's move, then the moves in UCI, the
//! opponent's first).

use alloc::vec::Vec;

/// The bundled puzzles, a line each: `FEN;moves;rating`.
pub static PUZZLES: &str = include_str!("../data/chess.txt");

/// A piece: its kind (1 pawn .. 6 king) and its colour.
pub const PAWN: u8 = 1;
pub const KNIGHT: u8 = 2;
pub const BISHOP: u8 = 3;
pub const ROOK: u8 = 4;
pub const QUEEN: u8 = 5;
pub const KING: u8 = 6;

/// A square's content: 0 empty, 1..=6 a white piece, 9..=14 a black one.
pub type Piece = u8;

#[must_use]
pub const fn kind(piece: Piece) -> u8 {
    piece & 7
}

#[must_use]
pub const fn is_black(piece: Piece) -> bool {
    piece & 8 != 0
}

/// A move: from and to as 0..64 (a8 = 0, h1 = 63), and what a pawn becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub from: u8,
    pub to: u8,
    pub promote: u8,
}

/// A position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Board {
    pub squares: [Piece; 64],
    pub black_to_move: bool,
    /// Castling rights: 1 white king side, 2 white queen side, 4 black king
    /// side, 8 black queen side.
    castling: u8,
    en_passant: Option<u8>,
}

const KNIGHT_STEPS: [(i8, i8); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];
const KING_STEPS: [(i8, i8); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// The square one step (`df` files, `dr` ranks down the array) from `sq`.
fn step(sq: u8, df: i8, dr: i8) -> Option<u8> {
    let (f, r) = (
        i8::try_from(sq % 8).ok()? + df,
        i8::try_from(sq / 8).ok()? + dr,
    );
    ((0..8).contains(&f) && (0..8).contains(&r)).then(|| u8::try_from(r * 8 + f).unwrap_or(0))
}

/// The name of a square: "e4".
#[must_use]
pub fn square_name(sq: u8) -> [u8; 2] {
    [b'a' + sq % 8, b'8' - sq / 8]
}

/// The square of a name such as "e4".
#[must_use]
pub fn square_of(name: &[u8]) -> Option<u8> {
    let (&f, &r) = (name.first()?, name.get(1)?);
    ((b'a'..=b'h').contains(&f) && (b'1'..=b'8').contains(&r)).then(|| (b'8' - r) * 8 + (f - b'a'))
}

impl Board {
    /// The position of a FEN; none for one that is malformed.
    #[must_use]
    pub fn from_fen(fen: &str) -> Option<Self> {
        let mut parts = fen.split_whitespace();
        let placement = parts.next()?;
        let mut squares = [0u8; 64];
        let (mut at, mut rows) = (0usize, 0);
        for c in placement.chars() {
            match c {
                '/' => {
                    rows += 1;
                    if at % 8 != 0 || at / 8 != rows {
                        return None;
                    }
                }
                '1'..='8' => at += usize::from(u8::try_from(c.to_digit(10)?).ok()?),
                _ => {
                    let kind = match c.to_ascii_lowercase() {
                        'p' => PAWN,
                        'n' => KNIGHT,
                        'b' => BISHOP,
                        'r' => ROOK,
                        'q' => QUEEN,
                        'k' => KING,
                        _ => return None,
                    };
                    *squares.get_mut(at)? = kind | if c.is_ascii_lowercase() { 8 } else { 0 };
                    at += 1;
                }
            }
        }
        if at != 64 {
            return None;
        }
        let black_to_move = match parts.next()? {
            "w" => false,
            "b" => true,
            _ => return None,
        };
        let mut castling = 0;
        for c in parts.next().unwrap_or("-").chars() {
            castling |= match c {
                'K' => 1,
                'Q' => 2,
                'k' => 4,
                'q' => 8,
                _ => 0,
            };
        }
        let en_passant = parts.next().and_then(|s| square_of(s.as_bytes()));
        Some(Self {
            squares,
            black_to_move,
            castling,
            en_passant,
        })
    }

    #[must_use]
    pub fn at(&self, sq: u8) -> Piece {
        self.squares.get(usize::from(sq)).copied().unwrap_or(0)
    }

    fn king_square(&self, black: bool) -> Option<u8> {
        (0..64u8).find(|s| self.at(*s) == KING | if black { 8 } else { 0 })
    }

    /// Whether `by` (black or white) attacks `sq`.
    fn attacked(&self, sq: u8, by_black: bool) -> bool {
        let own = if by_black { 8 } else { 0 };
        // Pawns attack down the array for white (towards rank 8 is up).
        let pawn_dr = if by_black { -1 } else { 1 };
        for df in [-1, 1] {
            if step(sq, df, pawn_dr).is_some_and(|s| self.at(s) == PAWN | own) {
                return true;
            }
        }
        for (df, dr) in KNIGHT_STEPS {
            if step(sq, df, dr).is_some_and(|s| self.at(s) == KNIGHT | own) {
                return true;
            }
        }
        for (df, dr) in KING_STEPS {
            if step(sq, df, dr).is_some_and(|s| self.at(s) == KING | own) {
                return true;
            }
            // Sliders along the same eight directions.
            let diagonal = df != 0 && dr != 0;
            let mut at = sq;
            while let Some(next) = step(at, df, dr) {
                let p = self.at(next);
                if p != 0 {
                    let k = kind(p);
                    if p & 8 == own && (k == QUEEN || k == if diagonal { BISHOP } else { ROOK }) {
                        return true;
                    }
                    break;
                }
                at = next;
            }
        }
        false
    }

    /// Whether the side to move is in check.
    #[must_use]
    pub fn in_check(&self) -> bool {
        self.king_square(self.black_to_move)
            .is_some_and(|k| self.attacked(k, !self.black_to_move))
    }

    fn pseudo_moves(&self, from: u8, out: &mut Vec<Move>) {
        let piece = self.at(from);
        if piece == 0 || is_black(piece) != self.black_to_move {
            return;
        }
        let black = self.black_to_move;
        let mut add = |to: u8, promote: u8| out.push(Move { from, to, promote });
        match kind(piece) {
            PAWN => self.pawn_moves(from, &mut add),
            KNIGHT | KING => {
                let steps = if kind(piece) == KNIGHT {
                    &KNIGHT_STEPS
                } else {
                    &KING_STEPS
                };
                for &(df, dr) in steps {
                    if let Some(to) = step(from, df, dr) {
                        let target = self.at(to);
                        if target == 0 || is_black(target) != black {
                            add(to, 0);
                        }
                    }
                }
                if kind(piece) == KING {
                    self.castles(from, &mut add);
                }
            }
            k => {
                for &(df, dr) in &KING_STEPS {
                    let diagonal = df != 0 && dr != 0;
                    if (k == BISHOP && !diagonal) || (k == ROOK && diagonal) {
                        continue;
                    }
                    let mut at = from;
                    while let Some(to) = step(at, df, dr) {
                        let target = self.at(to);
                        if target == 0 {
                            add(to, 0);
                        } else {
                            if is_black(target) != black {
                                add(to, 0);
                            }
                            break;
                        }
                        at = to;
                    }
                }
            }
        }
    }

    fn pawn_moves(&self, from: u8, add: &mut impl FnMut(u8, u8)) {
        let black = self.black_to_move;
        let dr = if black { 1 } else { -1 };
        let start = if black { 1 } else { 6 };
        let last = if black { 7 } else { 0 };
        let push = |to: u8, add: &mut dyn FnMut(u8, u8)| {
            if to / 8 == last {
                for p in [QUEEN, ROOK, BISHOP, KNIGHT] {
                    add(to, p);
                }
            } else {
                add(to, 0);
            }
        };
        if let Some(one) = step(from, 0, dr).filter(|s| self.at(*s) == 0) {
            push(one, add);
            if from / 8 == start
                && let Some(two) = step(one, 0, dr).filter(|s| self.at(*s) == 0)
            {
                add(two, 0);
            }
        }
        for df in [-1, 1] {
            if let Some(to) = step(from, df, dr) {
                let target = self.at(to);
                if (target != 0 && is_black(target) != black) || self.en_passant == Some(to) {
                    push(to, add);
                }
            }
        }
    }

    fn castles(&self, from: u8, add: &mut impl FnMut(u8, u8)) {
        let black = self.black_to_move;
        let (home, king_side, queen_side) = if black { (4, 4, 8) } else { (60, 1, 2) };
        if from != home || self.attacked(home, !black) {
            return;
        }
        let rook = ROOK | if black { 8 } else { 0 };
        let free = |squares: &[u8]| squares.iter().all(|s| self.at(*s) == 0);
        let safe = |squares: &[u8]| squares.iter().all(|s| !self.attacked(*s, !black));
        if self.castling & king_side != 0
            && self.at(home + 3) == rook
            && free(&[home + 1, home + 2])
            && safe(&[home + 1, home + 2])
        {
            add(home + 2, 0);
        }
        if self.castling & queen_side != 0
            && self.at(home - 4) == rook
            && free(&[home - 1, home - 2, home - 3])
            && safe(&[home - 1, home - 2])
        {
            add(home - 2, 0);
        }
    }

    /// The position after `m`, which must be a pseudo-legal move.
    #[must_use]
    pub fn after(&self, m: Move) -> Self {
        let mut next = self.clone();
        let piece = self.at(m.from);
        let (from, to) = (usize::from(m.from), usize::from(m.to));
        let captured = self.at(m.to);
        next.squares[from] = 0;
        next.squares[to] = if m.promote != 0 {
            m.promote | (piece & 8)
        } else {
            piece
        };
        if kind(piece) == PAWN
            && self.en_passant == Some(m.to)
            && captured == 0
            && m.from % 8 != m.to % 8
        {
            let taken = if is_black(piece) { to - 8 } else { to + 8 };
            next.squares[taken] = 0;
        }
        next.en_passant = (kind(piece) == PAWN && m.from.abs_diff(m.to) == 16)
            .then(|| u8::midpoint(m.from, m.to));
        if kind(piece) == KING && m.from.abs_diff(m.to) == 2 {
            let (rook_from, rook_to) = if m.to > m.from {
                (m.from + 3, m.from + 1)
            } else {
                (m.from - 4, m.from - 1)
            };
            next.squares[usize::from(rook_to)] = next.squares[usize::from(rook_from)];
            next.squares[usize::from(rook_from)] = 0;
        }
        // Rights are lost by a king or rook move, or a rook taken.
        for (sq, rights) in [(60, 3), (4, 12), (63, 1), (56, 2), (7, 4), (0, 8)] {
            if m.from == sq || m.to == sq {
                next.castling &= !rights;
            }
        }
        next.black_to_move = !self.black_to_move;
        next
    }

    /// The legal moves of the side to move.
    #[must_use]
    pub fn legal_moves(&self) -> Vec<Move> {
        let mut all = Vec::new();
        for from in 0..64u8 {
            self.pseudo_moves(from, &mut all);
        }
        all.retain(|m| {
            let after = self.after(*m);
            after
                .king_square(self.black_to_move)
                .is_some_and(|k| !after.attacked(k, !self.black_to_move))
        });
        all
    }

    /// The legal moves from `from`.
    #[must_use]
    pub fn moves_from(&self, from: u8) -> Vec<Move> {
        let mut moves = self.legal_moves();
        moves.retain(|m| m.from == from);
        moves
    }

    /// The legal move from `from` to `to` (a pawn reaching the last rank
    /// becomes `promote`, a queen where that is not told); none if illegal.
    #[must_use]
    pub fn find(&self, from: u8, to: u8, promote: u8) -> Option<Move> {
        let wanted = if promote == 0 { QUEEN } else { promote };
        self.moves_from(from)
            .into_iter()
            .find(|m| m.to == to && (m.promote == 0 || m.promote == wanted))
    }

    #[must_use]
    pub fn is_checkmate(&self) -> bool {
        self.in_check() && self.legal_moves().is_empty()
    }
}

/// A move in UCI ("e2e4", "e7e8q").
#[must_use]
pub fn parse_uci(text: &str) -> Option<Move> {
    let b = text.as_bytes();
    let promote = match b.get(4) {
        None => 0,
        Some(b'q') => QUEEN,
        Some(b'r') => ROOK,
        Some(b'b') => BISHOP,
        Some(b'n') => KNIGHT,
        Some(_) => return None,
    };
    Some(Move {
        from: square_of(b.get(0..2)?)?,
        to: square_of(b.get(2..4)?)?,
        promote,
    })
}

/// A puzzle: the position, the moves (the opponent's first) and the rating.
#[derive(Debug, Clone)]
pub struct Puzzle<'a> {
    pub fen: &'a str,
    pub moves: &'a str,
    pub rating: u32,
}

/// The `index`th bundled puzzle.
#[must_use]
pub fn puzzle(index: usize) -> Option<Puzzle<'static>> {
    let line = PUZZLES
        .lines()
        .filter(|l| !l.trim().is_empty())
        .nth(index)?;
    let mut parts = line.split(';');
    Some(Puzzle {
        fen: parts.next()?,
        moves: parts.next()?,
        rating: parts.next()?.trim().parse().ok()?,
    })
}

/// How many puzzles are bundled.
#[must_use]
pub fn count() -> usize {
    PUZZLES.lines().filter(|l| !l.trim().is_empty()).count()
}

/// A puzzle being solved: the position now, and how far the moves went.
#[derive(Debug, Clone)]
pub struct Play {
    pub board: Board,
    pub index: usize,
    /// How many of the puzzle's moves have been played.
    pub played: usize,
    pub moves: Vec<Move>,
    /// The last move played, for showing it.
    pub last: Option<Move>,
    pub wrong: bool,
    pub solved: bool,
}

/// What a move of the player did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Not a legal move.
    Illegal,
    /// A legal move that is not the one: it is taken back.
    Wrong,
    /// The right move; the opponent answered.
    Right,
    /// The right move, and the last: the puzzle is solved.
    Solved,
}

impl Play {
    /// The puzzle `index` (wrapping), with the opponent's first move played.
    #[must_use]
    pub fn new(index: usize) -> Option<Self> {
        let n = count();
        let index = index.checked_rem(n)?;
        let p = puzzle(index)?;
        let mut board = Board::from_fen(p.fen)?;
        let moves: Vec<Move> = p.moves.split_whitespace().filter_map(parse_uci).collect();
        let first = *moves.first()?;
        let m = board.find(first.from, first.to, first.promote)?;
        board = board.after(m);
        Some(Self {
            board,
            index,
            played: 1,
            moves,
            last: Some(m),
            wrong: false,
            solved: false,
        })
    }

    /// The player moves `from` to `to`.
    pub fn play(&mut self, from: u8, to: u8, promote: u8) -> Outcome {
        let Some(m) = self.board.find(from, to, promote) else {
            return Outcome::Illegal;
        };
        let Some(expected) = self.moves.get(self.played).copied() else {
            return Outcome::Illegal;
        };
        let after = self.board.after(m);
        // Any move that mates is as good as the one the puzzle has.
        let same = m.from == expected.from
            && m.to == expected.to
            && (m.promote == expected.promote || expected.promote == 0 || m.promote == 0);
        if !same && !after.is_checkmate() {
            self.wrong = true;
            return Outcome::Wrong;
        }
        self.wrong = false;
        self.board = after;
        self.last = Some(m);
        self.played += 1;
        if self.played >= self.moves.len() || self.board.is_checkmate() {
            self.solved = true;
            return Outcome::Solved;
        }
        // The opponent's answer.
        if let Some(reply) = self.moves.get(self.played).copied()
            && let Some(r) = self.board.find(reply.from, reply.to, reply.promote)
        {
            self.board = self.board.after(r);
            self.last = Some(r);
            self.played += 1;
        }
        Outcome::Right
    }

    /// The move the puzzle wants next.
    #[must_use]
    pub fn hint(&self) -> Option<Move> {
        self.moves.get(self.played).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

    fn count_moves(board: &Board, depth: u32) -> u64 {
        if depth == 0 {
            return 1;
        }
        board
            .legal_moves()
            .iter()
            .map(|m| count_moves(&board.after(*m), depth - 1))
            .sum()
    }

    #[test]
    fn move_generation_matches_known_perft_counts() {
        let start = Board::from_fen(START).unwrap();
        assert_eq!(count_moves(&start, 1), 20);
        assert_eq!(count_moves(&start, 2), 400);
        assert_eq!(count_moves(&start, 3), 8_902);
        // Castling, en passant, pins and promotions: "Kiwipete" and position 3.
        let kiwi =
            Board::from_fen("r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1")
                .unwrap();
        assert_eq!(count_moves(&kiwi, 1), 48);
        assert_eq!(count_moves(&kiwi, 2), 2_039);
        let three = Board::from_fen("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1").unwrap();
        assert_eq!(count_moves(&three, 3), 2_812);
        let promo = Board::from_fen("n1n5/PPPk4/8/8/8/8/4Kppp/5N1N b - - 0 1").unwrap();
        assert_eq!(count_moves(&promo, 2), 496);
    }

    #[test]
    fn a_move_is_validated_and_a_promotion_chosen() {
        let start = Board::from_fen(START).unwrap();
        let e2 = square_of(b"e2").unwrap();
        let e4 = square_of(b"e4").unwrap();
        let e5 = square_of(b"e5").unwrap();
        assert!(start.find(e2, e4, 0).is_some());
        assert!(start.find(e2, e5, 0).is_none(), "a pawn cannot jump three");
        assert!(start.find(e4, e5, 0).is_none(), "an empty square");
        let promo = Board::from_fen("8/P6k/8/8/8/8/8/K7 w - - 0 1").unwrap();
        let (a7, a8) = (square_of(b"a7").unwrap(), square_of(b"a8").unwrap());
        assert_eq!(promo.find(a7, a8, 0).unwrap().promote, QUEEN);
        assert_eq!(promo.find(a7, a8, KNIGHT).unwrap().promote, KNIGHT);
        assert_eq!(promo.moves_from(a7).len(), 4);
        let mate = Board::from_fen("6k1/5ppp/8/8/8/8/8/R5K1 w - - 0 1").unwrap();
        let after = mate.after(
            mate.find(square_of(b"a1").unwrap(), square_of(b"a8").unwrap(), 0)
                .unwrap(),
        );
        assert!(after.is_checkmate());
    }

    #[test]
    fn a_pinned_piece_cannot_move_and_check_must_be_answered() {
        let pinned = Board::from_fen("4r1k1/8/8/8/8/8/4B3/4K3 w - - 0 1").unwrap();
        assert!(pinned.moves_from(square_of(b"e2").unwrap()).is_empty());
        let check = Board::from_fen("4r1k1/8/8/8/8/8/R7/4K3 w - - 0 1").unwrap();
        assert!(check.in_check());
        assert_eq!(check.moves_from(square_of(b"a2").unwrap()).len(), 1);
    }

    #[test]
    fn bad_fens_are_none() {
        for fen in [
            "",
            "8/8/8 w - -",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBN w - - 0 1",
            "x7/8/8/8/8/8/8/8 w - -",
        ] {
            assert!(Board::from_fen(fen).is_none(), "{fen}");
        }
    }

    #[test]
    fn the_bundled_puzzles_are_legal_and_play_through() {
        assert!(count() >= 20);
        for index in 0..count() {
            let mut play = Play::new(index).unwrap_or_else(|| panic!("puzzle {index}"));
            let p = puzzle(index).unwrap();
            let moves: Vec<&str> = p.moves.split_whitespace().collect();
            assert_eq!(
                moves.len() % 2,
                0,
                "puzzle {index}: the opponent first, the player last"
            );
            let mut outcome = Outcome::Illegal;
            while let Some(m) = play.hint() {
                outcome = play.play(m.from, m.to, m.promote);
                assert!(
                    matches!(outcome, Outcome::Right | Outcome::Solved),
                    "puzzle {index}"
                );
                if outcome == Outcome::Solved {
                    break;
                }
            }
            assert_eq!(outcome, Outcome::Solved, "puzzle {index}");
            assert!(play.solved);
        }
    }

    #[test]
    fn a_wrong_move_is_refused_and_leaves_the_position() {
        let mut play = Play::new(0).unwrap();
        let before = play.board.clone();
        let wanted = play.hint().unwrap();
        let other = play
            .board
            .legal_moves()
            .into_iter()
            .find(|m| {
                (m.from, m.to) != (wanted.from, wanted.to) && !play.board.after(*m).is_checkmate()
            })
            .unwrap();
        assert_eq!(play.play(other.from, other.to, 0), Outcome::Wrong);
        assert!(play.wrong);
        assert_eq!(play.board, before);
        assert_eq!(play.play(0, 0, 0), Outcome::Illegal);
        assert!(!play.solved);
    }

    #[test]
    fn index_wraps() {
        assert_eq!(Play::new(count()).unwrap().index, 0);
    }
}
