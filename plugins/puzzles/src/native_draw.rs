//! Drawing the Puzzles screen: the header, the lists and the three boards. A
//! draw builds the operations of what lies in the band of the frame only (the
//! host draws a partial refresh in bands, and an operation outside the band
//! is never sent), so moving the cursor on a board costs a few cells, not
//! the board.

use crate::{
    chess,
    native_view::{ChessGame, CrossGame, Mode, ScreenView, Status, Zone},
    puz,
    state::Stats,
    sudoku::{self, Game},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Point, Rect,
    native::{
        Error, Host,
        callbacks::{DrawFrame, DrawPurpose},
        canvas::PartRole,
        fonts::{Font, FontRole, TextOrigin},
        formatting::Arg,
        snapshot::{self, Snapshot},
    },
};
use paperpaw_ui_kit::{
    Chips, Messages, Page, Paint, Style, header, paint,
    widget::{
        Align,
        shape::{fill_circle, stroke_circle},
        text::{Wrapped, line as one_line},
    },
};

/// Where things were drawn, for taps and for the parts to draw again.
#[derive(Debug)]
pub struct Geometry {
    pub x: u16,
    pub y: u16,
    pub cell: u16,
    pub cols: u8,
    pub rows: u8,
    /// The row of buttons under a board, and the line of text above them.
    pub bar: Rect,
    pub status: Rect,
    pub back: Option<Rect>,
    pub list: Rect,
    pub row_h: u16,
    pub visible: usize,
    chips: Option<Chips>,
}

impl Default for Geometry {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            cell: 0,
            cols: 0,
            rows: 0,
            bar: Rect::new(0, 0, 0, 0),
            status: Rect::new(0, 0, 0, 0),
            back: None,
            list: Rect::new(0, 0, 0, 0),
            row_h: 0,
            visible: 0,
            chips: None,
        }
    }
}

fn within(r: Rect, p: Point) -> bool {
    p.x >= i32::from(r.x)
        && p.y >= i32::from(r.y)
        && p.x < i32::from(r.x) + i32::from(r.width)
        && p.y < i32::from(r.y) + i32::from(r.height)
}

impl Geometry {
    /// The square `i` (row by row) of the board.
    #[must_use]
    pub fn cell_rect(&self, i: usize) -> Rect {
        let cols = usize::from(self.cols).max(1);
        let cell = u32::from(self.cell);
        let clamp = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
        let (c, r) = (
            u32::try_from(i % cols).unwrap_or(0),
            u32::try_from(i / cols).unwrap_or(0),
        );
        Rect::new(
            clamp(u32::from(self.x) + c * cell),
            clamp(u32::from(self.y) + r * cell),
            self.cell,
            self.cell,
        )
    }

    /// The square under `p`.
    #[must_use]
    pub fn cell_at(&self, p: Point) -> Option<usize> {
        let cell = i32::from(self.cell);
        if cell == 0 {
            return None;
        }
        let (dx, dy) = (p.x - i32::from(self.x), p.y - i32::from(self.y));
        if dx < 0
            || dy < 0
            || dx / cell >= i32::from(self.cols)
            || dy / cell >= i32::from(self.rows)
        {
            return None;
        }
        usize::try_from(dy / cell * i32::from(self.cols) + dx / cell).ok()
    }

    /// The `n`th visible row of the list.
    #[must_use]
    pub fn row_rect(&self, n: usize) -> Rect {
        let y = u32::from(self.list.y) + u32::from(self.row_h) * u32::try_from(n).unwrap_or(0);
        Rect::new(
            self.list.x,
            u16::try_from(y).unwrap_or(u16::MAX),
            self.list.width,
            self.row_h,
        )
    }

    /// The row of the list under `p`.
    #[must_use]
    pub fn row_at(&self, p: Point, first: usize, len: usize) -> Option<usize> {
        if !within(self.list, p) || self.row_h == 0 {
            return None;
        }
        let n = usize::try_from((p.y - i32::from(self.list.y)) / i32::from(self.row_h)).ok()?;
        let row = first + n;
        (n < self.visible && row < len).then_some(row)
    }

    /// The button under `p`.
    #[must_use]
    pub fn bar_at(&self, p: Point) -> Option<usize> {
        self.chips.as_ref().and_then(|c| c.hit(p))
    }
}

/// What a drawing is made with.
struct Env<'a, 'h, 'f> {
    host: &'a Host<'h>,
    /// The frame: ops are replayed in parts (a row of squares at a time) so
    /// that the list of operations in memory stays small.
    frame: core::cell::RefCell<&'a mut DrawFrame<'f>>,
    font: &'a Font<'a>,
    style: &'a Style,
    display: &'a Snapshot,
    /// The rows of the frame being drawn (empty: all of them).
    band: (i32, i32),
}

impl Env<'_, '_, '_> {
    /// Draws `ops` on the frame now.
    fn replay<'o>(&self, ops: impl IntoIterator<Item = &'o Paint>) -> Result<(), Error> {
        paint::replay(&mut self.frame.borrow_mut(), self.font, ops)
    }

    /// Draws what is in `ops` and empties it.
    fn flush(&self, ops: &mut Vec<Paint>) -> Result<(), Error> {
        self.replay(ops.iter())?;
        ops.clear();
        Ok(())
    }

    fn wanted(&self, r: Rect) -> bool {
        if self.band.0 >= self.band.1 {
            return true;
        }
        i32::from(r.y) < self.band.1 && i32::from(r.y) + i32::from(r.height) > self.band.0
    }
}

fn push(ops: &mut Vec<Paint>, op: Paint) -> Result<(), Error> {
    paint::push(ops, op)
}

fn number_text(value: u64) -> String {
    let mut out = String::new();
    sudoku::push_number(&mut out, value);
    out
}

impl ScreenView {
    fn title_key(&self) -> &'static str {
        match self.mode {
            Mode::Menu => "puzzles.title",
            Mode::Levels => "puzzles.title.levels",
            Mode::Sudoku if self.daily_open => "puzzles.title.daily",
            Mode::Sudoku => "puzzles.title.sudoku",
            Mode::Files => "puzzles.title.crosswords",
            Mode::Crossword => "puzzles.title.crossword",
            Mode::Clues => "puzzles.title.clues",
            Mode::Chess => "puzzles.title.chess",
            Mode::Stats => "puzzles.title.stats",
        }
    }

    /// Draws the header and what the mode shows.
    ///
    /// # Errors
    /// `NoMemory` or what the host says.
    pub fn draw(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        stats: &Stats,
    ) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = self.paint(host, frame, &display, &font, stats);
        font.release()?;
        result
    }

    fn paint(
        &mut self,
        host: &Host<'_>,
        frame: &mut DrawFrame<'_>,
        display: &Snapshot,
        font: &Font<'_>,
        stats: &Stats,
    ) -> Result<(), Error> {
        let mut style = Style::read(host, display, font)?;
        style.describing = paint::asking(frame);
        let (width, height) = (display.display_width(), display.display_height());
        let mut m = Messages::new()?;
        let mut page = Page::default();
        let title = m.copy(host, self.title_key())?;
        header::prepare(
            &mut page,
            host,
            font,
            &style,
            &mut m,
            &title,
            width,
            display.has_touch(),
            0,
            1,
        )?;
        let top = style.header.saturating_add(style.gap);
        let margin = style.margin.max(style.gap);
        let body = Rect::new(
            margin,
            top,
            width.saturating_sub(margin.saturating_mul(2)),
            height.saturating_sub(top).saturating_sub(margin),
        );
        let band = (
            frame.clip_top.saturating_sub(frame.rect.y),
            frame.clip_bottom.saturating_sub(frame.rect.y),
        );
        let env = Env {
            host,
            frame: core::cell::RefCell::new(frame),
            font,
            style: &style,
            display,
            band,
        };
        let mut ops: Vec<Paint> = Vec::new();
        push(
            &mut ops,
            Paint::Fill(
                Rect::new(0, top, width, height.saturating_sub(top)),
                style.paper,
            ),
        )?;
        match self.mode {
            Mode::Menu | Mode::Levels | Mode::Files | Mode::Clues | Mode::Stats => {
                self.list(&env, &mut ops, &mut m, body, stats)?;
            }
            Mode::Sudoku => self.sudoku(&env, &mut ops, &mut m, body)?,
            Mode::Crossword => self.crossword(&env, &mut ops, &mut m, body)?,
            Mode::Chess => self.chess(&env, &mut ops, &mut m, body, stats)?,
        }
        env.replay(page.paint.iter().chain(ops.iter()))?;
        self.geo.back = page.back;
        Ok(())
    }

    // Lists.

    fn build_rows(
        &self,
        host: &Host<'_>,
        m: &mut Messages,
        stats: &Stats,
    ) -> Result<Vec<(String, String)>, Error> {
        let mut rows = Vec::new();
        match self.mode {
            Mode::Menu => {
                let daily = if stats.daily_is(self.today) {
                    if stats.daily_done {
                        m.copy(host, "puzzles.detail.solved")?
                    } else {
                        m.format(
                            host,
                            "puzzles.detail.left",
                            &[Arg::Count(i64::from(stats.daily_left))],
                        )?
                    }
                } else {
                    String::new()
                };
                let sudoku = match self.free.as_ref() {
                    Some(g) if !g.solved() => m.copy(host, level_key(g.level))?,
                    _ => String::new(),
                };
                let total = chess::count();
                let chess = m.format(
                    host,
                    "puzzles.detail.chess",
                    &[
                        Arg::Int {
                            name: "number",
                            value: i64::from(stats.chess_next)
                                % i64::try_from(total.max(1)).unwrap_or(1)
                                + 1,
                        },
                        Arg::Int {
                            name: "total",
                            value: i64::try_from(total).unwrap_or(0),
                        },
                    ],
                )?;
                for (key, detail) in [
                    ("puzzles.menu.daily", daily),
                    ("puzzles.menu.sudoku", sudoku),
                    ("puzzles.menu.crossword", String::new()),
                    ("puzzles.menu.chess", chess),
                    ("puzzles.menu.stats", String::new()),
                ] {
                    rows.push((m.copy(host, key)?, detail));
                }
            }
            Mode::Levels => {
                for level in sudoku::Level::ALL {
                    rows.push((m.copy(host, level_key(level))?, String::new()));
                }
            }
            Mode::Files => {
                for name in &self.files {
                    rows.push((name.clone(), String::new()));
                }
            }
            Mode::Clues => self.clue_rows(host, m, &mut rows)?,
            _ => {
                for (key, n) in [
                    ("puzzles.stats.sudoku", stats.sudoku),
                    ("puzzles.stats.crossword", stats.crossword),
                    ("puzzles.stats.chess", stats.chess),
                    ("puzzles.stats.total", stats.total()),
                ] {
                    rows.push((m.copy(host, key)?, number_text(u64::from(n))));
                }
            }
        }
        Ok(rows)
    }

    fn clue_rows(
        &self,
        host: &Host<'_>,
        m: &mut Messages,
        rows: &mut Vec<(String, String)>,
    ) -> Result<(), Error> {
        let Some(game) = self.cross.as_ref() else {
            return Ok(());
        };
        let (a, d) = (
            m.copy(host, "puzzles.clue.a")?,
            m.copy(host, "puzzles.clue.d")?,
        );
        for (list, letter) in [(&game.cw.across, &a), (&game.cw.down, &d)] {
            for clue in list {
                let mut label = number_text(u64::from(clue.number));
                label.push_str(letter);
                label.push(' ');
                label.push_str(&clue.text);
                rows.push((label, String::new()));
            }
        }
        Ok(())
    }

    fn list(
        &mut self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        m: &mut Messages,
        body: Rect,
        stats: &Stats,
    ) -> Result<(), Error> {
        let style = env.style;
        if self.rows_stale || self.mode == Mode::Stats {
            self.rows = self.build_rows(env.host, m, stats)?;
            self.rows_stale = false;
        }
        let note_h = if matches!(self.mode, Mode::Clues | Mode::Files) {
            style.row.saturating_mul(3)
        } else {
            0
        };
        let list = Rect::new(
            body.x,
            body.y,
            body.width,
            body.height.saturating_sub(note_h),
        );
        let row_h = style.row.max(1);
        self.geo.list = list;
        self.geo.row_h = row_h;
        self.geo.visible = usize::from(list.height / row_h);
        let visible = self.geo.visible.max(1);
        self.first = self.first.min(self.rows.len().saturating_sub(visible));
        let focus = if self.mode == Mode::Stats {
            usize::MAX
        } else {
            self.focus
        };
        for (n, (label, detail)) in self.rows.iter().enumerate().skip(self.first).take(visible) {
            let rect = self.geo.row_rect(n - self.first);
            if env.wanted(rect) {
                list_row(env, ops, rect, (label, detail), n == focus)?;
            }
        }
        let note = Rect::new(body.x, list.y + list.height, body.width, note_h);
        if note_h > 0 && env.wanted(note) {
            self.list_note(env, ops, m, note)?;
        }
        Ok(())
    }

    /// Under a list: the whole clue that has the focus, or what to do where
    /// there are no files (or why a file was not read).
    fn list_note(
        &self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        m: &mut Messages,
        area: Rect,
    ) -> Result<(), Error> {
        let style = env.style;
        let text = if self.mode == Mode::Clues {
            self.cross
                .as_ref()
                .and_then(|g| {
                    let n = self.focus;
                    let across = g.cw.across.len();
                    if n < across {
                        g.cw.across.get(n)
                    } else {
                        g.cw.down.get(n - across)
                    }
                })
                .map(|c| c.text.clone())
                .unwrap_or_default()
        } else if let Some(fault) = self.fault {
            m.copy(env.host, fault_key(fault))?
        } else if self.files.is_empty() {
            m.copy(env.host, "puzzles.files.empty")?
        } else {
            String::new()
        };
        let wrapped = Wrapped::new(
            env.font,
            &text,
            style.small,
            (area.width, area.height),
            false,
        )?;
        wrapped.draw(ops, env.font, area, Align::Left, style.muted)?;
        Ok(())
    }

    // The bar under a board.

    /// Lays out the text line and the row of buttons under a board whose
    /// squares are `cols` by `rows`, `cap` pixels at most; returns the square.
    #[allow(
        clippy::too_many_arguments,
        reason = "the inputs of one layout: where, how big, what the buttons say"
    )]
    fn layout(
        &mut self,
        env: &Env<'_, '_, '_>,
        body: Rect,
        (cols, rows, cap): (u8, u8, u16),
        text_h: u16,
        labels: &[&str],
        reserve: &[&str],
    ) -> Result<(), Error> {
        let (style, gap) = (env.style, env.style.gap);
        let finger = Chips::finger(env.display);
        let bar_h = Chips::height_for(env.font, style, finger, body.width, reserve)?;
        let used = text_h
            .saturating_add(bar_h)
            .saturating_add(gap.saturating_mul(2));
        let room = body.height.saturating_sub(used);
        let cell = (body.width / u16::from(cols))
            .min(room / u16::from(rows))
            .min(cap)
            .max(1);
        let (w, h) = (cell * u16::from(cols), cell * u16::from(rows));
        self.geo.x = body.x + body.width.saturating_sub(w) / 2;
        self.geo.y = body.y;
        self.geo.cell = cell;
        self.geo.cols = cols;
        self.geo.rows = rows;
        let text_y = body.y + h + gap;
        self.geo.status = Rect::new(body.x, text_y, body.width, text_h);
        let bar_y = text_y + text_h + gap;
        self.geo.bar = Rect::new(body.x, bar_y, body.width, bar_h);
        let at = self.geo.bar;
        let mut chips = Chips::new(env.font, style, finger, at, labels)?;
        // Centred, where they stand in one row.
        if let (Some(first), Some(last)) = (chips.cell(0), chips.cell(labels.len() - 1)) {
            let span = last.x.saturating_add(last.width).saturating_sub(first.x);
            if span < at.width && last.y == first.y {
                let shifted = Rect::new(at.x + (at.width - span) / 2, at.y, span, at.height);
                chips = Chips::new(env.font, style, finger, shifted, labels)?;
            }
        }
        chips.set_describing(style.describing);
        self.geo.chips = Some(chips);
        Ok(())
    }

    fn bar_ops(&self, env: &Env<'_, '_, '_>, ops: &mut Vec<Paint>) -> Result<(), Error> {
        let Some(chips) = self.geo.chips.as_ref() else {
            return Ok(());
        };
        let focus = match self.zone {
            Zone::Bar(i) => Some(i),
            Zone::Board => None,
        };
        let chosen = if self.mode == Mode::Sudoku && self.notes_on && !self.palette {
            0
        } else {
            usize::MAX
        };
        let band = (env.band.0 < env.band.1).then(|| {
            Rect::new(
                0,
                u16::try_from(env.band.0.max(0)).unwrap_or(0),
                u16::MAX,
                u16::try_from((env.band.1 - env.band.0).max(1)).unwrap_or(1),
            )
        });
        chips.prepare_in(ops, env.font, chosen, focus, band)
    }

    /// One line of text in the text area under a board.
    fn status_line(
        &self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        text: &str,
    ) -> Result<(), Error> {
        let style = env.style;
        let area = self.geo.status;
        if !env.wanted(area) {
            return Ok(());
        }
        push(ops, Paint::Fill(area, style.paper))?;
        let wrapped = Wrapped::fitting(
            env.font,
            text,
            (style.text, style.small * 0.8),
            (area.width, area.height),
            false,
        )?;
        wrapped.draw(ops, env.font, area, Align::Center, style.ink)?;
        Ok(())
    }

    // Sudoku.

    fn sudoku(
        &mut self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        m: &mut Messages,
        body: Rect,
    ) -> Result<(), Error> {
        let Some(game) = self.game_ref() else {
            return Ok(());
        };
        let host = env.host;
        let (level, left) = (game.level, sudoku::CELLS - game.filled());
        let tools = [
            "puzzles.bar.notes",
            "puzzles.bar.check",
            "puzzles.bar.undo",
            "puzzles.bar.new",
        ];
        let mut labels: Vec<String> = Vec::new();
        if self.palette {
            for d in 1..=9u64 {
                labels.push(number_text(d));
            }
            labels.push(m.copy(host, "puzzles.bar.clear")?);
        } else {
            let n = if self.daily_open { 3 } else { 4 };
            for key in tools.iter().take(n) {
                labels.push(m.copy(host, key)?);
            }
        }
        let mut reserve: Vec<String> = (1..=9u64).map(number_text).collect();
        reserve.push(m.copy(host, "puzzles.bar.clear")?);
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let reserve_refs: Vec<&str> = reserve.iter().map(String::as_str).collect();
        let row = env.style.row;
        self.layout(env, body, (9, 9, 60), row, &refs, &reserve_refs)?;
        let level_name = m.copy(host, level_key(level))?;
        let status = match self.status {
            Status::Plain => m.format(
                host,
                "puzzles.status.sudoku",
                &[
                    Arg::Count(i64::try_from(left).unwrap_or(0)),
                    Arg::Text {
                        name: "level",
                        value: &level_name,
                    },
                ],
            )?,
            other => self.status_text(host, m, other)?,
        };
        self.sudoku_cells(env, ops)?;
        self.status_line(env, ops, &status)?;
        self.bar_ops(env, ops)
    }

    fn game_ref(&self) -> Option<&Game> {
        if self.daily_open {
            self.daily.as_deref()
        } else {
            self.free.as_deref()
        }
    }

    fn status_text(
        &self,
        host: &Host<'_>,
        m: &mut Messages,
        status: Status,
    ) -> Result<String, Error> {
        let key = match status {
            Status::Wrong(n) => {
                return m.format(
                    host,
                    "puzzles.status.wrong",
                    &[Arg::Count(i64::try_from(n).unwrap_or(0))],
                );
            }
            Status::NoMistakes => "puzzles.status.nomistakes",
            Status::Solved => "puzzles.status.solved",
            Status::NotThat => "puzzles.status.notthat",
            Status::Hint => "puzzles.status.hint",
            Status::Notes => "puzzles.status.notes",
            Status::Plain => return Ok(String::new()),
        };
        m.copy(host, key)
    }

    fn sudoku_cells(&self, env: &Env<'_, '_, '_>, ops: &mut Vec<Paint>) -> Result<(), Error> {
        let Some(game) = self.game_ref() else {
            return Ok(());
        };
        let (style, geo) = (env.style, &self.geo);
        let cell = geo.cell;
        let big = f32::from(cell) * 0.62;
        let small = (f32::from(cell) * 0.25).max(8.0);
        let area = Rect::new(geo.x, geo.y, cell * 9, cell * 9);
        if !env.wanted(area) {
            return Ok(());
        }
        push(ops, Paint::Fill(area, style.paper))?;
        let adv_big = env.font.measure_text(big, "0")?;
        let adv_small = env.font.measure_text(small, "0")?;
        for i in 0..sudoku::CELLS {
            if i > 0 && i % 9 == 0 {
                env.flush(ops)?;
            }
            let r = geo.cell_rect(i);
            if !env.wanted(r) {
                continue;
            }
            if game.marked && game.wrong(i) {
                push(ops, Paint::Stipple(r, style.ink))?;
            }
            let d = game.cur[i];
            if d != 0 {
                let text = number_text(u64::from(d));
                let origin = digit_origin(r, big, adv_big);
                push(
                    ops,
                    if game.is_given(i) {
                        Paint::Bold {
                            text,
                            size: big,
                            origin,
                            color: style.ink,
                        }
                    } else {
                        Paint::Text {
                            text,
                            size: big,
                            origin,
                            color: style.ink,
                            clip: None,
                        }
                    },
                )?;
            } else if game.notes[i] != 0 {
                notes(ops, r, game.notes[i], (small, adv_small), style.muted)?;
            }
        }
        grid_lines(ops, geo, (9, 9), 3, env, style.ink)?;
        if self.mode == Mode::Sudoku {
            let r = geo.cell_rect(self.cursor);
            if env.wanted(r) {
                let thick = if self.zone == Zone::Board { 4 } else { 2 };
                focus_frame(ops, r, thick, style.ink)?;
            }
        }
        Ok(())
    }

    // Crossword.

    fn crossword(
        &mut self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        m: &mut Messages,
        body: Rect,
    ) -> Result<(), Error> {
        let Some(game) = self.cross.as_ref() else {
            return Ok(());
        };
        let host = env.host;
        let (w, h) = (game.cw.width, game.cw.height);
        let dir = m.copy(
            host,
            if game.across {
                "puzzles.bar.across"
            } else {
                "puzzles.bar.down"
            },
        )?;
        let labels = [
            dir,
            m.copy(host, "puzzles.bar.clues")?,
            m.copy(host, "puzzles.bar.check")?,
            m.copy(host, "puzzles.bar.reveal")?,
        ];
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let line = paperpaw_ui_kit::geometry::line_px(env.font, env.style.small)?;
        self.layout(env, body, (w, h, 44), line.saturating_mul(3), &refs, &refs)?;
        let text = self.clue_text(host, m)?;
        self.crossword_cells(env, ops)?;
        self.clue_box(env, ops, &text)?;
        self.bar_ops(env, ops)
    }

    /// The line under a crossword: its status, else the clue under the cursor.
    fn clue_text(&self, host: &Host<'_>, m: &mut Messages) -> Result<String, Error> {
        if self.status != Status::Plain {
            return self.status_text(host, m, self.status);
        }
        let Some(game) = self.cross.as_ref() else {
            return Ok(String::new());
        };
        let Some(clue) = game.cw.word_at(game.cursor, game.across) else {
            return Ok(String::new());
        };
        let key = if game.across {
            "puzzles.clue.across"
        } else {
            "puzzles.clue.down"
        };
        let mut text = m.format(
            host,
            key,
            &[Arg::Int {
                name: "number",
                value: i64::from(clue.number),
            }],
        )?;
        text.push_str(": ");
        text.push_str(&clue.text);
        text.push_str(" (");
        sudoku::push_number(&mut text, u64::from(clue.len));
        text.push(')');
        Ok(text)
    }

    fn clue_box(
        &self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        text: &str,
    ) -> Result<(), Error> {
        let area = self.geo.status;
        if !env.wanted(area) {
            return Ok(());
        }
        push(ops, Paint::Fill(area, env.style.paper))?;
        let wrapped = Wrapped::fitting(
            env.font,
            text,
            (env.style.text, env.style.small * 0.75),
            (area.width, area.height),
            false,
        )?;
        wrapped.draw(ops, env.font, area, Align::Left, env.style.ink)?;
        Ok(())
    }

    fn crossword_cells(&self, env: &Env<'_, '_, '_>, ops: &mut Vec<Paint>) -> Result<(), Error> {
        let Some(game) = self.cross.as_ref() else {
            return Ok(());
        };
        let (style, geo) = (env.style, &self.geo);
        let cell = geo.cell;
        let size = f32::from(cell) * 0.6;
        let tiny = (f32::from(cell) * 0.28).max(7.0);
        let area = Rect::new(
            geo.x,
            geo.y,
            cell * u16::from(geo.cols),
            cell * u16::from(geo.rows),
        );
        if !env.wanted(area) {
            return Ok(());
        }
        push(ops, Paint::Fill(area, style.paper))?;
        let word: Vec<usize> = game
            .cw
            .word_at(game.cursor, game.across)
            .map(|c| game.cw.cells_of(c, game.across).collect())
            .unwrap_or_default();
        let adv = env.font.measure_text(size, "M")?;
        for i in 0..game.cw.letters.len() {
            if i > 0 && i % usize::from(geo.cols).max(1) == 0 {
                env.flush(ops)?;
            }
            let r = geo.cell_rect(i);
            if !env.wanted(r) {
                continue;
            }
            cross_cell(ops, env, (game, i, r), (size, tiny, adv), word.contains(&i))?;
        }
        grid_lines(
            ops,
            geo,
            (usize::from(geo.cols), usize::from(geo.rows)),
            1000,
            env,
            style.ink,
        )?;
        let r = geo.cell_rect(game.cursor);
        if env.wanted(r) {
            focus_frame(
                ops,
                r,
                if self.zone == Zone::Board { 4 } else { 2 },
                style.ink,
            )?;
        }
        Ok(())
    }

    // Chess.

    fn chess(
        &mut self,
        env: &Env<'_, '_, '_>,
        ops: &mut Vec<Paint>,
        m: &mut Messages,
        body: Rect,
        stats: &Stats,
    ) -> Result<(), Error> {
        let Some(game) = self.chess.as_ref() else {
            return Ok(());
        };
        let host = env.host;
        let text = self.chess_text(host, m, game, stats)?;
        let labels = [
            m.copy(host, "puzzles.bar.hint")?,
            m.copy(host, "puzzles.bar.next")?,
            m.copy(host, "puzzles.bar.restart")?,
        ];
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        let row = env.style.row;
        self.layout(env, body, (8, 8, 64), row, &refs, &refs)?;
        self.chess_cells(env, ops)?;
        self.status_line(env, ops, &text)?;
        self.bar_ops(env, ops)
    }

    fn chess_text(
        &self,
        host: &Host<'_>,
        m: &mut Messages,
        game: &ChessGame,
        _stats: &Stats,
    ) -> Result<String, Error> {
        if self.status != Status::Plain {
            return self.status_text(host, m, self.status);
        }
        let key = if game.play.board.black_to_move {
            "puzzles.status.chess.black"
        } else {
            "puzzles.status.chess.white"
        };
        m.format(
            host,
            key,
            &[
                Arg::Int {
                    name: "number",
                    value: i64::try_from(game.play.index + 1).unwrap_or(0),
                },
                Arg::Int {
                    name: "total",
                    value: i64::try_from(chess::count()).unwrap_or(0),
                },
                Arg::Int {
                    name: "rating",
                    value: game.rating(),
                },
            ],
        )
    }

    fn chess_cells(&self, env: &Env<'_, '_, '_>, ops: &mut Vec<Paint>) -> Result<(), Error> {
        let Some(game) = self.chess.as_ref() else {
            return Ok(());
        };
        let (style, geo) = (env.style, &self.geo);
        let cell = geo.cell;
        let area = Rect::new(geo.x, geo.y, cell * 8, cell * 8);
        if !env.wanted(area) {
            return Ok(());
        }
        push(ops, Paint::Fill(area, style.paper))?;
        let size = f32::from(cell) * 0.5;
        let adv = env.font.measure_text(size, "W")?;
        for d in 0..64 {
            if d > 0 && d % 8 == 0 {
                env.flush(ops)?;
            }
            let r = geo.cell_rect(d);
            if !env.wanted(r) {
                continue;
            }
            let square = game.square(d);
            if (d / 8 + d % 8) % 2 == 1 {
                push(ops, Paint::Stipple(r, style.ink))?;
            }
            let piece = game.play.board.at(square);
            if piece != 0 {
                chess_piece(ops, r, piece, (size, adv), style)?;
            }
            if game.targets.contains(&square) {
                target_mark(ops, r, piece != 0, style.ink, style.paper)?;
            }
            if game.selected == Some(square) {
                focus_frame(ops, r, 3, style.ink)?;
            }
            if let Some(hint) = game.hint
                && (hint.from == square || hint.to == square)
            {
                focus_frame(ops, r, 2, style.ink)?;
            }
        }
        grid_lines(ops, geo, (8, 8), 1000, env, style.ink)?;
        let r = geo.cell_rect(game.display(game.cursor));
        if env.wanted(r) && !game.play.solved {
            focus_frame(
                ops,
                r,
                if self.zone == Zone::Board { 4 } else { 2 },
                style.ink,
            )?;
        }
        Ok(())
    }
}

impl ChessGame {
    fn rating(&self) -> i64 {
        chess::puzzle(self.play.index).map_or(0, |p| i64::from(p.rating))
    }
}

fn level_key(level: sudoku::Level) -> &'static str {
    match level {
        sudoku::Level::Easy => "puzzles.level.easy",
        sudoku::Level::Medium => "puzzles.level.medium",
        sudoku::Level::Hard => "puzzles.level.hard",
    }
}

fn fault_key(fault: puz::Fault) -> &'static str {
    match fault {
        puz::Fault::Format => "puzzles.fault.format",
        puz::Fault::Scrambled => "puzzles.fault.scrambled",
        puz::Fault::Large => "puzzles.fault.large",
    }
}

/// A row of a list: the label at the left, the detail at the right; the
/// focused row is a bar of ink.
fn list_row(
    env: &Env<'_, '_, '_>,
    ops: &mut Vec<Paint>,
    rect: Rect,
    (label, detail): (&str, &str),
    focused: bool,
) -> Result<(), Error> {
    let style = env.style;
    let (ink, muted) = if focused {
        push(
            ops,
            Paint::Focus(rect, u32::from(style.row / 5), None, style.ink),
        )?;
        (style.paper, style.paper)
    } else {
        push(
            ops,
            Paint::Fill(
                Rect::new(rect.x, rect.y + rect.height - 1, rect.width, 1),
                style.rule,
            ),
        )?;
        (style.ink, style.muted)
    };
    paint::describe(ops, style.describing, PartRole::Row, rect, focused, false)?;
    let pad = style.gap.max(6);
    let inner = Rect::new(
        rect.x + pad,
        rect.y,
        rect.width.saturating_sub(pad * 2),
        rect.height,
    );
    let mut label_room = inner;
    if !detail.is_empty() {
        let wide = env.font.measure_text(style.small, detail)?;
        let wide = u16::try_from(wide as u32).unwrap_or(0).saturating_add(pad);
        let detail_area = Rect::new(
            inner.x + inner.width.saturating_sub(wide),
            inner.y,
            wide,
            inner.height,
        );
        one_line(
            ops,
            env.font,
            detail_area,
            Align::Right,
            (style.small, style.small),
            detail,
            muted,
        )?;
        label_room.width = inner.width.saturating_sub(wide);
    }
    one_line(
        ops,
        env.font,
        label_room,
        Align::Left,
        (style.text, style.small),
        label,
        ink,
    )?;
    Ok(())
}

/// Where a digit of `size` with advance `adv` starts in `cell`.
fn digit_origin(cell: Rect, size: f32, adv: f32) -> TextOrigin {
    let x = f32::from(cell.x) + (f32::from(cell.width) - adv) / 2.0;
    let baseline = i32::from(cell.y) + i32::from(cell.height) / 2 + (size * 0.36) as i32;
    TextOrigin::new(x, baseline)
}

/// The pencil marks of a cell, in three rows of three.
fn notes(
    ops: &mut Vec<Paint>,
    cell: Rect,
    marks: u16,
    (size, adv): (f32, f32),
    color: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let third = cell.width / 3;
    for k in 0..9u16 {
        if marks & (1 << k) == 0 {
            continue;
        }
        let part = Rect::new(
            cell.x + (k % 3) * third,
            cell.y + (k / 3) * third,
            third,
            third,
        );
        let origin = digit_origin(part, size, adv);
        push(
            ops,
            Paint::Text {
                text: number_text(u64::from(k) + 1),
                size,
                origin,
                color,
                clip: None,
            },
        )?;
    }
    Ok(())
}

/// The lines of a board: every `bold`th a thick one.
fn grid_lines(
    ops: &mut Vec<Paint>,
    geo: &Geometry,
    (cols, rows): (usize, usize),
    bold: usize,
    env: &Env<'_, '_, '_>,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let cell = geo.cell;
    let (w, h) = (
        cell * u16::try_from(cols).unwrap_or(0),
        cell * u16::try_from(rows).unwrap_or(0),
    );
    for k in 0..=cols {
        let t = if k % bold == 0 { 3 } else { 1 };
        let x = geo.x + cell * u16::try_from(k).unwrap_or(0);
        let r = Rect::new(x.saturating_sub(t / 2), geo.y, t, h);
        push(ops, Paint::Fill(r, ink))?;
    }
    for k in 0..=rows {
        let t = if k % bold == 0 { 3 } else { 1 };
        let y = geo.y + cell * u16::try_from(k).unwrap_or(0);
        let r = Rect::new(geo.x, y.saturating_sub(t / 2), w, t);
        if env.wanted(r) {
            push(ops, Paint::Fill(r, ink))?;
        }
    }
    Ok(())
}

/// A frame inside `r`, `thick` pixels wide.
fn focus_frame(
    ops: &mut Vec<Paint>,
    r: Rect,
    thick: u16,
    ink: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let inner = Rect::new(
        r.x + 1,
        r.y + 1,
        r.width.saturating_sub(2),
        r.height.saturating_sub(2),
    );
    push(ops, Paint::Focus(inner, 4, Some(u32::from(thick)), ink))
}

fn cross_cell(
    ops: &mut Vec<Paint>,
    env: &Env<'_, '_, '_>,
    (game, i, r): (&CrossGame, usize, Rect),
    (size, tiny, adv): (f32, f32, f32),
    in_word: bool,
) -> Result<(), Error> {
    let style = env.style;
    if game.cw.is_block_at(i) {
        return push(ops, Paint::Fill(r, style.ink));
    }
    if in_word {
        push(ops, Paint::Stipple(r, style.rule))?;
    }
    let number = game.cw.numbers.get(i).copied().unwrap_or(0);
    if number != 0 {
        let origin = TextOrigin::new(f32::from(r.x) + 2.0, i32::from(r.y) + tiny as i32 + 1);
        push(
            ops,
            Paint::Text {
                text: number_text(u64::from(number)),
                size: tiny,
                origin,
                color: style.muted,
                clip: None,
            },
        )?;
    }
    let letter = game.cw.letters.get(i).copied().unwrap_or(puz::EMPTY);
    if letter != puz::EMPTY && letter != puz::BLOCK {
        let mut text = String::new();
        text.push(char::from(letter));
        let x = f32::from(r.x) + (f32::from(r.width) - adv * 0.8) / 2.0;
        let baseline = i32::from(r.y) + i32::from(r.height) * 3 / 4 + 1;
        push(
            ops,
            Paint::Text {
                text,
                size,
                origin: TextOrigin::new(x, baseline),
                color: style.ink,
                clip: None,
            },
        )?;
    }
    if game.marked && game.cw.wrong(i) {
        let (a, b) = (
            Point::new(i32::from(r.x) + 2, i32::from(r.y) + i32::from(r.height) - 2),
            Point::new(i32::from(r.x) + i32::from(r.width) - 2, i32::from(r.y) + 2),
        );
        push(
            ops,
            Paint::Line {
                from: a,
                to: b,
                width: 2,
                color: style.ink,
            },
        )?;
    }
    Ok(())
}

/// A chess piece: a letter in a disc, outlined for white and black for black.
fn chess_piece(
    ops: &mut Vec<Paint>,
    r: Rect,
    piece: chess::Piece,
    (size, adv): (f32, f32),
    style: &Style,
) -> Result<(), Error> {
    let centre = Point::new(
        i32::from(r.x) + i32::from(r.width) / 2,
        i32::from(r.y) + i32::from(r.height) / 2,
    );
    let radius = u32::from(r.width) * 42 / 100;
    let black = chess::is_black(piece);
    // White pieces are light and black ones dark on any theme.
    let (light, dark) = if style.dark {
        (style.ink, style.paper)
    } else {
        (style.paper, style.ink)
    };
    if black {
        fill_circle(ops, centre, radius, dark)?;
        stroke_circle(ops, centre, radius, 2, light)?;
    } else {
        fill_circle(ops, centre, radius, light)?;
        stroke_circle(ops, centre, radius, 3, dark)?;
    }
    let letter = match chess::kind(piece) {
        chess::KING => 'K',
        chess::QUEEN => 'Q',
        chess::ROOK => 'R',
        chess::BISHOP => 'B',
        chess::KNIGHT => 'N',
        _ => 'P',
    };
    let mut text = String::new();
    text.push(letter);
    let x = centre.x as f32 - adv * 0.5;
    let baseline = centre.y + (size * 0.36) as i32;
    push(
        ops,
        Paint::Bold {
            text,
            size,
            origin: TextOrigin::new(x, baseline),
            color: if black { light } else { dark },
        },
    )
}

/// Where the selected piece may go: a dot on an empty square, a ring round a
/// piece that would be taken.
fn target_mark(
    ops: &mut Vec<Paint>,
    r: Rect,
    capture: bool,
    ink: paperpaw_sdk::Color,
    paper: paperpaw_sdk::Color,
) -> Result<(), Error> {
    let centre = Point::new(
        i32::from(r.x) + i32::from(r.width) / 2,
        i32::from(r.y) + i32::from(r.height) / 2,
    );
    if capture {
        stroke_circle(ops, centre, u32::from(r.width) * 46 / 100, 4, ink)
    } else {
        let radius = u32::from(r.width) / 6;
        fill_circle(ops, centre, radius + 2, paper)?;
        fill_circle(ops, centre, radius, ink)
    }
}
