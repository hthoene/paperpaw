//! The *Add* screen's gallery with the host: the widgets of one kind measured,
//! laid out (`native_gallery`), made and composed as the page shown needs, with
//! the page keys, taps and the text rows of the cells that cannot be drawn.
use paperpaw_sdk::widgets::categories;
use paperpaw_ui_kit::{
    icons, list,
    widget::{look::Align, text as label},
};

use super::*;

/// The host as the gallery's stage: its instances are the screen's own, and the
/// screen's frame is what they are drawn into.
struct Live<'a, 'f> {
    host: &'a Host<'a>,
    ui: &'a Ui,
    frame: &'a mut DrawFrame<'f>,
}

impl Stage for Live<'_, '_> {
    type Handle = Made;

    fn make(&mut self, id: &str, area: Rect) -> Result<Made, Error> {
        let made = Made::make(self.host, self.ui, id, measure::PLACE)?;
        if let Err(error) = made.place(self.host, self.ui, area) {
            made.release(self.host, self.ui);
            return Err(error);
        }
        Ok(made)
    }

    fn release(&mut self, widget: Made) {
        widget.release(self.host, self.ui);
    }

    fn compose(&mut self, widget: &Made, area: Rect) -> Result<(), Error> {
        widget.draw(self.host, self.ui, self.frame, area)
    }
}

/// What a gallery page is drawn from.
struct Scene<'a> {
    title: &'a str,
    /// The kind shown (its icon heads the text rows).
    kind: &'a str,
    entries: &'a [Entry],
    rows: &'a [Row],
    focus: usize,
    page: usize,
}

impl Scene<'_> {
    fn entry(&self, row: usize) -> Option<&Entry> {
        match self.rows.get(row)? {
            Row::Add(index) => self.entries.get(*index),
            _ => None,
        }
    }
}

/// The id of the widget of a row.
fn widget_of<'a>(rows: &[Row], entries: &'a [Entry], row: usize) -> Option<&'a str> {
    match rows.get(row)? {
        Row::Add(index) => match &entries.get(*index)?.what {
            What::Widget(id) => Some(id.as_str()),
            What::Tool(_) => None,
        },
        _ => None,
    }
}

impl AddScreen {
    /// Lays the gallery out where it is still to be: the widgets measured, as
    /// they would be on the page.
    ///
    /// # Errors
    /// Allocation and host errors.
    pub(super) fn ensure(&mut self, host: &Host<'_>, ui: &Ui) -> Result<(), Error> {
        if matches!(self.mode, Mode::Pending) {
            self.mode = match self.lay_out(host, ui)? {
                Some((laid, stay)) => {
                    self.arrows_stay = stay;
                    Mode::Gallery(laid)
                }
                None => Mode::List,
            };
        }
        Ok(())
    }

    /// The gallery, and whether the arrow keys stay on its page; `None` where
    /// the host measures nothing (the X4 until its stack gate): the list.
    fn lay_out(&self, host: &Host<'_>, ui: &Ui) -> Result<Option<(Gallery<Made>, bool)>, Error> {
        let mut shown = Shown::current(host)?;
        let mut widgets = Vec::new();
        for row in 0..self.rows.len() {
            let Some(id) = widget_of(&self.rows, &self.entries, row) else {
                continue;
            };
            match measure::measure_widget(host, ui, id, measure::PLACE, &shown.frame.geometry) {
                Ok(Some((rows, least))) => shown.src.measured(id, rows, least)?,
                Ok(None) => {}
                Err(Error::Unsupported) => return Ok(None),
                Err(Error::NoMemory) => return Err(Error::NoMemory),
                // Not measured (refused, trapped): the grid's default of a row.
                Err(_) => {}
            }
            let (columns, rows, _) = shown.src.widget(&shown.frame.geometry, id);
            push(&mut widgets, (row, (columns, rows)))?;
        }
        let (area, title_h, gap) = Self::room(host)?;
        let previews = gallery::lay_out(&widgets, &shown.frame.geometry, area, title_h, gap)?;
        Ok(Some((Gallery::new(previews), shown.src.arrows_stay)))
    }

    /// Where the previews go: below the header, the height of a name and the
    /// gap.
    fn room(host: &Host<'_>) -> Result<(Rect, u16, u16), Error> {
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let read = (|| {
            let style = Style::read(host, &display, &font)?;
            let name = geometry::line_px(&font, style.title)?.saturating_add(style.gap / 2);
            Ok((style.header.saturating_add(style.gap), name, style.gap))
        })();
        let released = font.release();
        let (top, name, gap) = read?;
        released?;
        let area = Rect::new(
            0,
            top,
            display.display_width(),
            display.display_height().saturating_sub(top),
        );
        Ok((area, name, gap))
    }

    /// Draws the gallery's page with the focus. `false` where the host composes
    /// nothing: the gallery is the list from now on, and the caller draws it.
    ///
    /// # Errors
    /// Allocation, font, canvas and host errors.
    pub(super) fn draw_gallery(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        frame: &mut DrawFrame<'_>,
    ) -> Result<bool, Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(true);
        }
        let Self {
            mode,
            focus,
            title,
            kind,
            entries,
            rows,
            header,
            ..
        } = self;
        let Mode::Gallery(laid) = mode else {
            return Ok(false);
        };
        let scene = Scene {
            title,
            kind: kind.as_deref().unwrap_or(""),
            entries,
            rows,
            focus: focus.focus(),
            page: laid.page_of(focus.focus()),
        };
        let mut stage = Live { host, ui, frame };
        let made = laid.show(&mut stage, scene.page, |row| widget_of(rows, entries, row));
        let drawn = match made {
            Ok(()) => Self::paint(laid, &mut stage, &scene, header),
            Err(error) => Err(error),
        };
        match drawn {
            Ok(Drawn::Rows(_)) => Ok(true),
            Ok(Drawn::List) | Err(Error::Unsupported) => {
                laid.release(&mut stage);
                *mode = Mode::List;
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    /// The page painted: the header, each name and the frame round its widget,
    /// the widgets composed in, then a text row for each cell that is none.
    fn paint(
        laid: &Gallery<Made>,
        stage: &mut Live<'_, '_>,
        scene: &Scene<'_>,
        header: &mut Page,
    ) -> Result<Drawn, Error> {
        let host = stage.host;
        let display = snapshot::capture_snapshot(host)?;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let drawn = Self::paint_with(&font, &display, laid, stage, scene, header);
        let released = font.release();
        let drawn = drawn?;
        released?;
        Ok(drawn)
    }

    fn paint_with(
        font: &Font<'_>,
        display: &snapshot::Snapshot,
        laid: &Gallery<Made>,
        stage: &mut Live<'_, '_>,
        scene: &Scene<'_>,
        header: &mut Page,
    ) -> Result<Drawn, Error> {
        let host = stage.host;
        let style = Style::read(host, display, font)?;
        let width = display.display_width();
        let top = style.header.saturating_add(style.gap);
        let mut head = Page::default();
        header::prepare(
            &mut head,
            host,
            font,
            &style,
            &mut Messages::new()?,
            scene.title,
            width,
            display.has_touch(),
            scene.page,
            laid.pages(),
        )?;
        let mut body = Vec::new();
        let below = display.display_height().saturating_sub(top);
        paint::push(
            &mut body,
            Paint::Fill(Rect::new(0, top, width, below), style.paper),
        )?;
        for (_, preview) in laid.on_page(scene.page) {
            let name = scene.entry(preview.row).map_or("", |entry| &entry.title);
            let ink = style.ink;
            label::line(
                &mut body,
                font,
                preview.title,
                Align::Left,
                (style.title, style.small),
                name,
                ink,
            )?;
            ring(&mut body, &style, preview.area, preview.row == scene.focus)?;
        }
        paint::replay(stage.frame, font, head.paint.iter().chain(body.iter()))?;
        let drawn = laid.compose(stage, scene.page);
        if let Drawn::Rows(missing) = &drawn {
            Self::paint_rows(stage.frame, font, host, &style, laid, scene, missing)?;
        }
        head.paint = Vec::new();
        *header = head;
        Ok(drawn)
    }

    /// The cells that are no widget drawn as a text row: icon, name, cells.
    fn paint_rows(
        frame: &mut DrawFrame<'_>,
        font: &Font<'_>,
        host: &Host<'_>,
        style: &Style,
        laid: &Gallery<Made>,
        scene: &Scene<'_>,
        missing: &[usize],
    ) -> Result<(), Error> {
        if missing.is_empty() {
            return Ok(());
        }
        let icon_font = icons::font(host)?;
        let glyph = icons::glyph(host, categories::icon(scene.kind).unwrap_or("puzzle"))?;
        let mut body = Vec::new();
        for &number in missing {
            let Some(preview) = laid.previews().get(number) else {
                continue;
            };
            let name = scene.entry(preview.row).map_or("", |entry| &entry.title);
            let focused = preview.row == scene.focus;
            let rect = preview.area;
            let side = rect.height.min(style.row);
            let mut content = rect;
            if let (Some(icon_font), Some(glyph)) = (icon_font.as_ref(), glyph.as_ref()) {
                let square = Rect::new(rect.x, rect.y, side, side);
                icons::draw_centred(
                    &mut body,
                    icon_font,
                    glyph,
                    style.title,
                    square,
                    list::ink(style, focused),
                )?;
                content = Rect::new(
                    rect.x.saturating_add(side),
                    rect.y,
                    rect.width.saturating_sub(side),
                    rect.height,
                );
            }
            let mut digits = ([0u8; 20], [0u8; 20]);
            let cells = cells_text(preview.cells, &mut digits)?;
            let row = Rect::new(
                content.x,
                content.y,
                content.width,
                content.height.min(style.row),
            );
            list::row(&mut body, font, style, row, name, &cells, focused)?;
        }
        paint::replay_icons(frame, font, icon_font.as_ref(), body.iter())
    }

    /// An input on the gallery: the keys move among the widgets or by a page,
    /// *Confirm* or a tap on a widget adds it, a tap in the header goes back or
    /// turns the page.
    ///
    /// # Errors
    /// Allocation and host errors; nothing is staged then.
    pub(super) fn gallery_input(
        &mut self,
        host: &Host<'_>,
        ui: &Ui,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        let Mode::Gallery(laid) = &self.mode else {
            return Ok(Handled::No);
        };
        let focus = self.focus.focus().min(self.rows.len().saturating_sub(1));
        let page = laid.page_of(focus);
        let last = self.rows.len().saturating_sub(1);
        let to = match *input {
            Input::Action { action, .. } => match action {
                "core.confirm" if !self.rows.is_empty() => {
                    self.choose(host, ui, focus)?;
                    return Ok(Handled::Yes);
                }
                "core.down" | "core.next" | "core.right" => self
                    .step(laid, page, focus, true)
                    .unwrap_or(focus.saturating_add(1)),
                "core.up" | "core.previous" | "core.left" => self
                    .step(laid, page, focus, false)
                    .unwrap_or(focus.saturating_sub(1)),
                "core.next-page" => Self::turned(laid, page, true).unwrap_or(focus),
                "core.prev-page" => Self::turned(laid, page, false).unwrap_or(focus),
                _ => return Ok(Handled::No),
            },
            Input::Gesture {
                position,
                kind: InputGestureKind::Tap,
                ..
            } => {
                if let Some(action) = self.header.action(position) {
                    let mut scratch = [0; 128];
                    CommandSender::new(host, &mut scratch).run_action(action)?;
                    return Ok(Handled::Yes);
                }
                let Some(row) = laid.row_at(page, position) else {
                    return Ok(Handled::No);
                };
                self.focus.set(row);
                self.choose(host, ui, row)?;
                return Ok(Handled::Yes);
            }
            _ => return Ok(Handled::No),
        };
        let to = to.min(last);
        if to != focus {
            let mut scratch = [0; 64];
            CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
            self.focus.set(to);
        }
        Ok(Handled::Yes)
    }

    /// The row a key step goes to where the arrows stay on the page: round
    /// among its widgets.
    fn step(&self, laid: &Gallery<Made>, page: usize, focus: usize, down: bool) -> Option<usize> {
        if !self.arrows_stay {
            return None;
        }
        let count = laid.rows_on(page).count();
        let at = laid.rows_on(page).position(|row| row == focus)?;
        let to = if down {
            (at + 1) % count
        } else {
            (at + count - 1) % count
        };
        laid.rows_on(page).nth(to)
    }

    /// The first widget of the next or the previous page (the same where there
    /// is none).
    fn turned(laid: &Gallery<Made>, page: usize, forward: bool) -> Option<usize> {
        let to = if forward {
            page.saturating_add(1).min(laid.pages() - 1)
        } else {
            page.saturating_sub(1)
        };
        laid.first_row(to)
    }
}

/// The frame round a widget: strong where it has the focus, a faint line where
/// not, so that its size shows.
fn ring(paint: &mut Vec<Paint>, style: &Style, area: Rect, focused: bool) -> Result<(), Error> {
    let pad = style.gap / 2;
    let rect = Rect::new(
        area.x.saturating_sub(pad),
        area.y.saturating_sub(pad),
        area.width.saturating_add(2 * pad),
        area.height.saturating_add(2 * pad),
    );
    let (thickness, color) = if focused {
        (u32::from(style.line) * 3, style.ink)
    } else {
        (u32::from(style.line), style.rule)
    };
    paint::push(
        paint,
        Paint::Focus(rect, style.corner, Some(thickness), color),
    )
}

/// The cells a widget takes as text: `4×2`.
fn cells_text(
    (columns, rows): (u8, u8),
    digits: &mut ([u8; 20], [u8; 20]),
) -> Result<String, Error> {
    let mut text = String::new();
    text.try_reserve_exact(8).map_err(|_| Error::NoMemory)?;
    text.push_str(decimal(usize::from(columns), &mut digits.0));
    text.push('×');
    text.push_str(decimal(usize::from(rows), &mut digits.1));
    Ok(text)
}
