//! The reading screen's side of `reader.decorations@1` (`native_decorations`):
//! a page's marks asked once its words are made, drawn with it, and a tap
//! on a mark that takes taps handed to the plugin whose mark it is.
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host,
        callbacks::{Input, InputGestureKind},
        canvas::Canvas,
        stages::Stage,
    },
    reading::WordBox,
};

use super::{Body, Open, View};
use crate::native_decorations::{self as decorations, Decorations};
use crate::native_marks::{CanvasPen, Gauge};
use crate::stages::reader_decorations_v1 as stage;
use paperpaw_sdk::native::document::Documents;

impl Open {
    /// Asks the page's marks; made words are the page's (never fails the
    /// draw: a refusal leaves the page unmarked). Where no plugin subscribed,
    /// it is not asked again for this book until the screen is back on top.
    pub(super) fn decorate(&mut self, host: &Host<'_>) {
        self.decorations = None;
        if !self.ask_marks || self.undecorated || NO_STAGES.load(Ordering::Relaxed) {
            return;
        }
        match ask(host, &self.words, (&self.path, &self.language)) {
            Ok(Some(found)) => self.decorations = Some(found),
            Ok(None) => self.undecorated = true,
            Err(_) => {}
        }
    }

    /// The screen is back on top: a word added meanwhile shows (the page is
    /// asked again at its next draw).
    pub(super) fn decorate_again(&mut self) {
        self.undecorated = false;
        self.words = Vec::new();
    }

    /// Draws the page's backgrounds, before its text.
    pub(super) fn draw_behind(
        &self,
        canvas: &Canvas<'_>,
        gauge: Gauge,
        at: (i32, i32),
    ) -> Result<(), Error> {
        match &self.decorations {
            Some(found) => {
                decorations::draw_behind(&mut CanvasPen(canvas), gauge, found, &self.words, at)
            }
            None => Ok(()),
        }
    }

    /// Draws the page's lines and frames, after its text.
    pub(super) fn draw_over(
        &self,
        canvas: &Canvas<'_>,
        gauge: Gauge,
        at: (i32, i32),
    ) -> Result<(), Error> {
        match &self.decorations {
            Some(found) => {
                decorations::draw_over(&mut CanvasPen(canvas), gauge, found, &self.words, at)
            }
            None => Ok(()),
        }
    }

    /// A tap on a mark that takes taps, while nothing is chosen: the stage is
    /// run with it, and the plugin whose role it is takes it or not.
    ///
    /// # Errors
    /// Allocation and the host's refusals.
    pub(super) fn decoration_tap(
        &self,
        host: &Host<'_>,
        input: &Input<'_>,
    ) -> Result<Option<Handled>, Error> {
        let Input::Gesture {
            position,
            kind: InputGestureKind::Tap,
            ..
        } = input
        else {
            return Ok(None);
        };
        if self.marking.is_active() {
            return Ok(None);
        }
        let Some(found) = &self.decorations else {
            return Ok(None);
        };
        let Some((look, covered)) = found.tapped(&self.words, position.x, position.y) else {
            return Ok(None);
        };
        let input = decorations::tap_input(look, &covered, &self.path)?;
        let mut out = reply_buffer()?;
        let stage = Stage::new(host, stage::KEY)?;
        let mut run = stage.run(&input, &mut out);
        while let Some(reply) = run.next()? {
            if reply.status().is_ok()
                && decorations::taken(&look.role, reply.subscriber(), reply.payload())
            {
                return Ok(Some(Handled::Yes));
            }
        }
        Ok(None)
    }
}

impl View {
    /// The first page of a book is asked only after it is shown, in a step
    /// of the background work, never in the open (its buffers are freed by
    /// then, #248).
    #[must_use]
    pub fn marks_due(&self) -> bool {
        matches!(&self.body, Body::Open(open) if !open.ask_marks)
    }

    /// The step: the page on show is asked, and drawn again where it got marks.
    pub fn marks_step(&mut self, host: &Host<'_>, docs: &Documents) {
        let Body::Open(open) = &mut self.body else {
            return;
        };
        if open.ask_marks {
            return;
        }
        open.ask_marks = true;
        if open.words.is_empty() {
            return;
        }
        open.decorate(host);
        if open
            .decorations
            .as_ref()
            .is_some_and(|found| !found.is_unmarked())
        {
            open.redraw_page(
                host,
                docs,
                paperpaw_sdk::native::commands::RedrawMode::Partial,
            )
            .ok();
        }
    }
}

/// The device runs no stages (the X4 until its stack gate): it will not
/// change without a firmware update, so it is asked once a session.
static NO_STAGES: AtomicBool = AtomicBool::new(false);

/// The marks `words` are given, fired in whole runs; `None` where no plugin
/// subscribed (or the device runs no stages yet).
fn ask(
    host: &Host<'_>,
    words: &[WordBox],
    page: (&str, &str),
) -> Result<Option<Decorations>, Error> {
    let stage = Stage::new(host, stage::KEY)?;
    // A device that runs no stages (the X4 until its stack gate) says so
    // before it looks at any buffer: nothing is encoded or taken there.
    // Elsewhere this empty probe is `ERR_NO_SPACE`, never `ERR_NOT_FOUND`:
    // "nobody subscribed" is only learned from a real fire at cursor 0.
    if matches!(stage.run(&[], &mut []).next(), Err(Error::Unsupported)) {
        NO_STAGES.store(true, Ordering::Relaxed);
        return Ok(None);
    }
    let mut found = Decorations::new(words)?;
    if found.is_empty() {
        return Ok(Some(found));
    }
    // Taken after the first input is made (its texts dropped), #248.
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(fire) = found.fire(words, page, from)? {
        if out.is_empty() {
            out = reply_buffer()?;
        }
        let mut anyone = false;
        let mut run = stage.run(&fire.input, &mut out);
        loop {
            match run.next() {
                Ok(Some(reply)) => {
                    anyone = true;
                    if reply.status().is_ok() {
                        found.absorb(&fire, reply.subscriber(), reply.payload());
                    }
                }
                Ok(None) => break,
                // No stages on this device yet (the X4's stack gate).
                Err(Error::Unsupported) if from == 0 => {
                    NO_STAGES.store(true, Ordering::Relaxed);
                    return Ok(None);
                }
                // What the others answered stays.
                Err(_) => break,
            }
        }
        if !anyone && from == 0 {
            return Ok(None);
        }
        from = fire.first_run + fire.runs;
    }
    Ok(Some(found))
}

fn reply_buffer() -> Result<Vec<u8>, Error> {
    let room = Stage::out_len(stage::MAX_REPLY as u32);
    let mut out = Vec::new();
    out.try_reserve_exact(room).map_err(|_| Error::NoMemory)?;
    out.resize(room, 0);
    Ok(out)
}
