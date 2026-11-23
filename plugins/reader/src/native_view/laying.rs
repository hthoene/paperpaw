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
        self.with_faces(host, setup, |faces| {
            let page = self.lay_with(host, docs, faces, setup, start, area)?;
            Ok((page, faces.take_failed()))
        })
    }

    /// The start of the line `at` falls in, the text set as the page is: a
    /// page shown from a place far from here (a search hit, a highlight, a
    /// percentage) starts with its whole line, as in Beta 3 (#441).
    pub(super) fn line_start(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        at: (u32, u32),
    ) -> Result<(u32, u32), Error> {
        let area = self.area(setup).ok_or(Error::NoSpace)?;
        self.with_faces(host, setup, |faces| {
            let start = self.with_context(host, docs, faces, setup, at, area, |cx| {
                paperpaw_sdk::layout::line_start(cx, Locator::new(at.0, at.1))
            })?;
            Ok((start.section, start.offset))
        })
    }

    /// `then` with the faces of `setup`, the host's fonts let go of after it.
    fn with_faces<R>(
        &self,
        host: &Host<'_>,
        setup: &Setup,
        then: impl FnOnce(&Faces<&HostFonts<'_>>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let tables = self.settings.tables();
        let fonts = HostFonts::acquire(host)?;
        let done = Faces::with_tables(&fonts, setup.base(), setup.reading.spacing_factor(), tables)
            .and_then(|faces| {
                let faces = faces
                    .word_spacing(setup.reading.word_factor())
                    .with_letters(setup.reading.letters);
                then(&faces)
            });
        let released = fonts.release();
        done.and_then(|done| released.map(|()| done))
    }

    /// Lays out the page after the one on show and keeps it for the next
    /// forward turn (`forward`), which then only takes it. Nothing happens
    /// where it is kept already, the page on show is not drawn with the
    /// current settings, it is the last, or the host refuses.
    pub(super) fn lay_ahead(&mut self, host: &Host<'_>, docs: &Documents) {
        let Some(page) = &self.page else {
            return;
        };
        let end = (page.end.section, page.end.offset);
        if self.ahead.is_some() || page.is_past_end() || end <= self.at {
            return;
        }
        let Ok(setup) = self.settings.setup(host) else {
            return;
        };
        let key = self.key(&setup);
        if self.laid != Some(key) {
            return;
        }
        if let Onto::Page(laid) = onto(self.lay(host, docs, &setup, end)) {
            self.ahead = Some(Ahead {
                from: self.at,
                key,
                page: *laid,
            });
        }
    }

    /// The page's last word, where the layout broke it at the page's end, made
    /// whole with the first word of the next page: laid out for it, only when a
    /// word is being chosen or the touch is on that word (a page turn does not
    /// lay the next page out twice). A refusal leaves the word as it is.
    pub(super) fn complete_end(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        setup: &Setup,
        input: &Input<'_>,
    ) {
        let Some(page) = self.page.as_ref() else {
            return;
        };
        let wanted = match *input {
            Input::Action { action, .. } => {
                action == crate::native_marking::SELECT || self.marking.chosen()
            }
            Input::Gesture { position, .. } => self.words.last().is_some_and(|word| {
                native_wordbox::word_at(core::slice::from_ref(word), position.x, position.y)
                    .is_some()
            }),
            _ => false,
        };
        if !wanted || !native_wordbox::ends_open(&self.words, page) {
            return;
        }
        let end = page.end;
        let Ok((next, _)) = self.lay_flagged(host, docs, setup, (end.section, end.offset)) else {
            return;
        };
        if let Ok(Some((rest, at))) = native_wordbox::first_word(&next) {
            native_wordbox::join_open_end(&mut self.words, &rest, at).ok();
        }
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
        self.with_context(host, docs, faces, setup, start, area, |cx| {
            layout_page(cx, Locator::new(start.0, start.1), f32::from(area.height))
        })
    }

    /// `lay` with the context the page at `start` is laid out in (the book's
    /// blocks through the filters and text stages, the style, the hyphenation).
    #[expect(
        clippy::too_many_arguments,
        reason = "lay_with's, and the one thing done in its context"
    )]
    fn with_context<R>(
        &self,
        host: &Host<'_>,
        docs: &Documents,
        faces: &Faces<&HostFonts<'_>>,
        setup: &Setup,
        start: (u32, u32),
        area: native_pages::Box,
        lay: impl Fn(&LayoutContext<'_>) -> Result<R, paperpaw_sdk::document::DocumentError>,
    ) -> Result<R, Error> {
        let document = HostDocument::new(host, docs, self.id, self.sections, self.whole_pages)?
            .filtered(&self.filters, &self.path)
            .layered(&self.layers);
        // The blocks a plugin's text stage changes are asked before the layout.
        self.prepare_layers(host, &document, start);
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
            let (left, right) = (u32::from(left), u32::from(right));
            let ask = |word: &str, places: &mut [u8]| {
                paperpaw_sdk::native::text::hyphenate(
                    host,
                    &self.language,
                    word,
                    left,
                    right,
                    places,
                )
            };
            breaks.places(&ask, &self.language, (left, right), word, out);
        };
        let cx = LayoutContext::new(&document, &NoStorage, f32::from(area.width), &style, faces)
            .hyphenate(on.then_some(&hyphenate as Hyphenate<'_>));
        let lay = || {
            lay(&cx).map_err(|error| match document_error_kind(&error) {
                Some(kind) => kind,
                None => Error::Invalid,
            })
        };
        let laid = lay()?;
        // A block read beyond what was asked ahead: asked now, laid out once more.
        if self.prepare_missed(host, &document) {
            return lay();
        }
        Ok(laid)
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
        let tables = self.settings.tables();
        let faces =
            Faces::with_tables(fonts, setup.base(), setup.reading.spacing_factor(), tables)?
                .word_spacing(setup.reading.word_factor())
                .with_letters(setup.reading.letters);
        // A turned screen or a changed reading setting (the host may draw before
        // the plugin is told): the page is laid out again, its trail found anew.
        let key = self.key(setup);
        if self.page.is_some() && self.laid != Some(key) {
            self.page = None;
            self.ahead = None;
            self.trail.clear();
            self.layers.forget_pages();
        }
        if self.page.is_none() {
            let page = self.lay_with(host, docs, &faces, setup, self.at, area)?;
            // Laid out only here: drawn in one pass, with the pictures it has
            // ids for; the others are asked for after the draw (#438).
            self.pictures = pictures::Pass::Whole;
            self.missing = pictures::Missing::default();
            self.missing.due = !page.images.is_empty();
            self.page = Some(page);
            self.words = Vec::new();
            self.laid = Some(key);
        }
        if self.zoom.is_some() && self.draw_zoom(host, docs, frame)? {
            return Ok(());
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
        // Asked once for the page (and for each sentence read aloud where the
        // plugin follows it); a page kept is not asked again.
        // The whole content box: a layer's region leaves the text a part of it.
        let full = self.page_area(setup).unwrap_or(area);
        if self.ask_marks {
            self.ask_layer(host, full);
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
        let looks = || self.settings.mark_looks(host);
        let painted = Painted::new(looks, &self.marks, &self.words, hit, gauge).unwrap_or_default();
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
        // What a plugin's page layer draws, over the text.
        let at = (rect.x + i32::from(full.x), rect.y + i32::from(full.y));
        self.draw_layer(&canvas, (fonts, &look), at);
        // Other plugins' marks under the reader's own, as Beta 3 drew them.
        self.draw_over(&canvas, gauge, (rect.x, rect.y)).ok();
        painted.over(&mut CanvasPen(&canvas)).ok();
        self.draw_ribbon(&canvas, rect, (&full, setup.display.display_dpi()))?;
        self.paint_status(host, ui, frame, (setup, &full))?;
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
