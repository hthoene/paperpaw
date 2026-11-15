//! Laying out the page on show and drawing it: the `Open` methods that ask
//! the host for the fonts, set the text with the reading settings and paint
//! the page, its marks and its status.
use super::*;

impl Open {
    /// The page starting at `start`, laid out with `setup`.
    pub(super) fn lay(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        start: (u32, u32),
    ) -> Result<Page, Error> {
        self.lay_flagged(host, docs, setup, start)
            .map(|(page, _)| page)
    }

    /// The page starting at `start`, and whether the host refused a font read
    /// meanwhile (letters measured 0 wide: the page is not to be trusted).
    pub(super) fn lay_flagged(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        start: (u32, u32),
    ) -> Result<(Page, bool), Error> {
        let area = self.area(setup).ok_or(Error::NoSpace)?;
        let tables = self.settings.tables(setup.reading.letters);
        let fonts = HostFonts::acquire(host)?;
        let page = Faces::with_tables(&fonts, setup.base(), setup.reading.spacing_factor(), tables)
            .and_then(|faces| {
                let faces = faces
                    .word_spacing(setup.reading.word_factor())
                    .with_letters(setup.reading.letters);
                let page = self.lay_with(host, docs, &faces, setup, start, area)?;
                Ok((page, faces.take_failed()))
            });
        let released = fonts.release();
        page.and_then(|page| released.map(|()| page))
    }

    pub(super) fn lay_with(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        faces: &Faces<&HostFonts<'_>>,
        setup: &Setup,
        start: (u32, u32),
        area: native_pages::Box,
    ) -> Result<Page, Error> {
        let document = HostDocument::new(host, docs, self.id, self.sections, self.whole_pages)?
            .filtered(&self.filters, &self.path);
        let style = setup
            .style(faces.line_height())
            .script_options(native_pages::script_options(
                &self.language,
                setup.reading.scripts,
            ));
        // Words broken at the end of a line where the setting is on: the places
        // are the host's table for the book's language, kept for the words met.
        let (on, left, right) = setup.reading.hyphens;
        let breaks = if on {
            self.settings.breaks()
        } else {
            Breaks::default()
        };
        let hyphenate = |word: &str, out: &mut dyn FnMut(usize)| {
            breaks.places(
                host,
                &self.language,
                (u32::from(left), u32::from(right)),
                word,
                out,
            );
        };
        let cx = LayoutContext::new(&document, &NoStorage, f32::from(area.width), &style, faces)
            .hyphenate(on.then_some(&hyphenate as Hyphenate<'_>));
        layout_page(&cx, Locator::new(start.0, start.1), f32::from(area.height)).map_err(|error| {
            match document_error_kind(&error) {
                Some(kind) => kind,
                None => Error::Invalid,
            }
        })
    }

    pub(super) fn draw(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        let setup = self.settings.setup(host)?;
        let area = self.area(&setup).ok_or(Error::NoSpace)?;
        let fonts = HostFonts::acquire(host)?;
        let drawn = self.draw_with(host, (docs, ui), frame, (&setup, &fonts, area));
        let released = fonts.release();
        drawn.and(released)
    }

    pub(super) fn draw_with(
        &mut self,
        host: &Host<'_>,
        (docs, ui): (&Documents, Option<&Ui>),
        frame: &mut DrawFrame<'_>,
        (setup, fonts, area): (&Setup, &HostFonts<'_>, native_pages::Box),
    ) -> Result<(), Error> {
        let tables = self.settings.tables(setup.reading.letters);
        let faces =
            Faces::with_tables(fonts, setup.base(), setup.reading.spacing_factor(), tables)?
                .word_spacing(setup.reading.word_factor())
                .with_letters(setup.reading.letters);
        // A turned screen or a changed reading setting (the host may draw before
        // the plugin is told): the page is laid out again, its trail found anew.
        let key = self.key(setup);
        if self.page.is_some() && self.laid != Some(key) {
            self.page = None;
            self.trail.clear();
        }
        if self.page.is_none() {
            let page = self.lay_with(host, docs, &faces, setup, self.at, area)?;
            // Laid out only here: drawn in one pass.
            self.pictures = pictures::Pass::Whole;
            self.page = Some(page);
            self.words = Vec::new();
            self.laid = Some(key);
        }
        if frame.pictures_only() {
            return self.draw_pictures_only(host, frame, (setup, fonts, area));
        }
        let page = self.page.as_ref().ok_or(Error::State)?;
        if self.words.is_empty() {
            self.words = native_wordbox::word_boxes(&faces, page, area)?;
            // Highlights of another conversion that this page has the text of.
            self.marks.accept_foreign(&self.words);
            self.decorate(host);
        }
        let page = self.page.as_ref().ok_or(Error::State)?;
        let rect = frame.rect;
        let band = (frame.clip_top, frame.clip_bottom);
        let follow_up = pictures::follow_up(frame);
        let canvas = frame.draw_canvas()?;
        canvas.fill_rectangle(rect, Color::WHITE)?;
        let look = self
            .settings
            .look(host, setup.display.display_gray_levels())?;
        let mut ink = HostInk::new(fonts, &canvas, Color::BLACK, look).clipped(band.0, band.1);
        let origin = (
            rect.x as f32 + f32::from(area.x),
            rect.y as f32 + f32::from(area.y),
        );
        // The highlights and the search hit: behind the words, over them.
        let hit = self.marks.hit_on(self.at);
        // A mark never fails the page: a refusal leaves the text without it.
        let gauge = Gauge::of(
            setup.display.display_dpi(),
            setup.display.display_gray_levels(),
            setup.display.display_is_colour(),
        );
        let painted = Painted::new(host, &self.marks, &self.words, hit, gauge).unwrap_or_default();
        self.draw_behind(&canvas, gauge, (rect.x, rect.y)).ok();
        painted.behind(&mut CanvasPen(&canvas)).ok();
        let pictures = self.pictures.drawing(follow_up);
        draw_page(
            &mut ink,
            &faces,
            page,
            origin,
            (self.mark.clone(), pictures),
        )?;
        // Other plugins' marks under the reader's own, as Beta 3 drew them.
        self.draw_over(&canvas, gauge, (rect.x, rect.y)).ok();
        painted.over(&mut CanvasPen(&canvas)).ok();
        self.draw_ribbon(&canvas, rect, (&area, setup.display.display_dpi()))?;
        self.paint_status(host, ui, frame, (setup, &area))?;
        let ctx = Ctx::new(
            Locator::new(self.at.0, self.at.1),
            &self.words,
            &self.path,
            &self.title,
            "",
            area,
        )
        .doc(DocRef::new(docs, self.id, self.sections, self.whole_pages))
        .book(Book {
            highlights: self.marks.highlights(),
            library: None,
            conversion: self.conversion,
            progress: self.progress.as_ref(),
            stored_ppm: self.stored_ppm,
        });
        self.marking.draw_overlay(host, &ctx, frame)
    }
}
