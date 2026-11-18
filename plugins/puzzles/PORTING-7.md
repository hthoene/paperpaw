# Puzzles on interface 7

Status: **complete.** A new plugin, never published on interface 6; version 1.0.0, no interface change (#137).
It follows `plugins/pomodoro` (publish set: `tools/plugin-files/plugins.txt`, `tools/plugin-files7/complete.txt`
and `src/bin/puzzles.rs`; linked and shipped off in the emulator, `LINKED_STORE_PLUGINS`).

## Where things are

- `declarations.rs` (built into the image by `build.rs`): the screen, the actions `open` and `daily`, the Tools
  entry (order 85), the widget *Puzzle of the day* with the shared look, the events subscribed. No settings.
- `src/sudoku.rs`: the solver (counts solutions up to two; iterative, a stack of 81 entries, no recursion),
  the generator (a shuffled full grid, squares taken out in random order, one only if the search proves one
  solution is left within a budget of 4,000 steps), the levels (40, 32 and 26 clues aimed at: the level is the
  number of clues, not a technique), `Game` (values, pencil marks, undo of 48 moves, check) and its kept text.
- `src/puz.rs`: `.puz` (Across Lite; scrambled ones refused) and `ipuz` (a scanner over the JSON text, no tree)
  into `Crossword`: the numbering is worked out from the grid, the clues are matched by number; at most 25 by 25
  squares and 16 KiB a file; check, reveal, fill from typed letters, the kept letters.
- `src/chess.rs`: FEN, legal moves (check, castling, en passant, promotion; perft-tested), the bundled puzzles
  (`data/chess.txt`, 27 from the Lichess puzzle database, CC0: FEN, UCI moves, rating) and `Play` (the opponent's
  first move is played, then the player's; any mate also counts).
- `src/state.rs`: `Stats` and the names of the kept files (`docs/data-files.md`).
- `src/native_view.rs`: what each key and tap means; `src/native_draw.rs`: the drawing, by band; `src/native_widgets.rs`;
  `src/native_plugin.rs`: the glue (clock, kept files, the card's folder through `core.files`, the keyboard question).

## How it keeps the e-ink calm

- No work timers, nothing runs without a screen open; the widget is told only when the puzzle of the day changes.
- A cursor move queues partial redraws of the two squares it touches (and, in a crossword, of the two words);
  a digit redraws its square and the line under the board; the draw builds the operations of the cells in the
  band being drawn only (an operation outside it is never sent). A full refresh only for a new puzzle.
- Working state: a sudoku about 0.7 KB, a crossword (25 by 25) about 2 KB and its clues' text, a chess puzzle
  about 0.2 KB; a crossword file is read whole once (16 KiB at most) and freed after parsing.
- Keys are mapped as actions: left/right and up/down (the page keys), Select, Back, hold Select or Menu for the
  buttons under the board. Chess: left/right cycle through the pieces that can move, then through the targets of
  the one selected. Touch: a tap on a square, a button or a row.

## Not done on purpose

- No crossword is bundled (the folder `puzzles` on the card is empty by default).
- A sudoku's level is the clue count; the generator does not grade by technique.
- A promotion in a chess puzzle is a queen unless the puzzle's own move says otherwise.
- The generator runs in the callback (an easy one takes a fraction of a second; a hard one a few on the X4).

## Tests

- `sudoku`: uniqueness of generated puzzles, the solver against a known puzzle, undo, the kept text.
- `puz`: a `.puz` and an `ipuz` of three by three, refusal of damaged, scrambled and large files, typing, check, reveal.
- `chess`: perft counts (start, Kiwipete, two more), move validation, promotion, pins; every bundled puzzle plays through.
- `tests/native_linked_entry.rs`; `tools/pps/puzzles.pps` runs the screen on the emulator (menu, sudoku, kept game, puzzle of the day, chess, a crossword from `tools/pps/fixtures/tiny.puz`, statistics, the widget): 9 tests, all PASS on the Linux emulator; not run on a device.
