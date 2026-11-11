//! The reading screen's side of `reader.decorations@1` (`native_decorations`):
//! a page's marks asked once its words are made, drawn with it, and a tap
//! on a mark that takes taps handed to the plugin whose mark it is.
use alloc::vec::Vec;
use paperpaw_sdk::{
    Color, Handled,
    native::{
        Error, Host,
        callbacks::{FrameRect, Input, InputGestureKind},
        canvas::Canvas,
        stages::Stage,
    },
    reading::WordBox,
};

use super::Open;
use crate::native_decorations::{self as decorations, Decorations, Paint};
use crate::stages::reader_decorations_v1 as stage;

impl Open {
    /// Asks the page's marks; made words are the page's (never fails the
    /// draw: a refusal leaves the page unmarked). Where no plugin subscribed,
    /// it is not asked again for this book until the screen is back on top.
    pub(super) fn decorate(&mut self, host: &Host<'_>) {
        self.decorations = None;
        if self.undecorated {
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
    pub(super) fn draw_behind(&self, canvas: &Canvas<'_>, at: (i32, i32)) -> Result<(), Error> {
        match &self.decorations {
            Some(found) => decorations::draw_behind(&mut OnCanvas(canvas), found, &self.words, at),
            None => Ok(()),
        }
    }

    /// Draws the page's lines and frames, after its text.
    pub(super) fn draw_over(
        &self,
        canvas: &Canvas<'_>,
        at: (i32, i32),
        dpi: u16,
    ) -> Result<(), Error> {
        match &self.decorations {
            Some(found) => {
                decorations::draw_over(&mut OnCanvas(canvas), found, &self.words, at, dpi)
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
    if matches!(stage.run(&[], &mut []).next(), Err(Error::Unsupported)) {
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
                Err(Error::Unsupported) if from == 0 => return Ok(None),
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

/// The host's canvas as [`Paint`].
struct OnCanvas<'c, 'a>(&'c Canvas<'a>);

impl Paint for OnCanvas<'_, '_> {
    fn fill(&mut self, x: i32, y: i32, width: u32, height: u32, color: Color) -> Result<(), Error> {
        self.0
            .fill_rectangle(FrameRect::new(x, y, width, height), color)
    }
}
