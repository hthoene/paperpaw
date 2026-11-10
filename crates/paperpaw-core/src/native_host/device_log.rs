//! The device log: one line of diagnostics a shell may show (`KernelBuilder::device_log`).
//!
//! It is for the host's own findings (a format skipped, a cache left
//! unfinished), never for plugin content: a line carries plugin ids and
//! numbers (reasons, counts) only, no plugin-supplied text, paths, titles or
//! secrets. A device without a sink drops the line (nothing is formatted);
//! a line is cut at 120 bytes on a character boundary. The sink is a plain
//! `fn` and keeps no state of its own (a shell that needs state uses a
//! `static`). Nothing allocates. A
//! call site logs at most one line per sync or job.
use crate::abi7::context::Frame;
use core::fmt::{self, Write};

/// The longest line, in bytes.
pub(crate) const LINE_MAX: usize = 120;

/// One line for `frame`: `args` are rendered only when the frame has a sink
/// ([`Frame::log`]). For the services of the host (todo 390): at most one
/// line per call site per sync or job, plugin ids and numbers only.
#[allow(dead_code, reason = "the document service logs through it (todo 390)")]
pub(crate) fn frame_line(frame: &dyn Frame, args: fmt::Arguments<'_>) {
    frame.log(args);
}

/// The shell's sink, if it gave one.
#[derive(Clone, Copy, Default)]
pub(crate) struct DeviceLog(Option<fn(&str)>);

impl fmt::Debug for DeviceLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DeviceLog").field(&self.0.is_some()).finish()
    }
}

/// A fixed buffer that takes whole characters only.
struct Line {
    bytes: [u8; LINE_MAX],
    used: usize,
}

impl Write for Line {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let room = LINE_MAX - self.used;
        let mut take = text.len().min(room);
        while !text.is_char_boundary(take) {
            take -= 1;
        }
        self.bytes[self.used..self.used + take].copy_from_slice(&text.as_bytes()[..take]);
        self.used += take;
        // Cut: the rest of the line is not wanted either.
        if take < text.len() {
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl DeviceLog {
    pub(crate) const fn new(sink: Option<fn(&str)>) -> Self {
        Self(sink)
    }

    /// Writes one line, cut at [`LINE_MAX`] bytes; dropped without a sink.
    pub(crate) fn line(self, args: fmt::Arguments<'_>) {
        let Some(sink) = self.0 else {
            return;
        };
        let mut line = Line {
            bytes: [0; LINE_MAX],
            used: 0,
        };
        // A full buffer is the cut; any other failure leaves what was written.
        let _ = line.write_fmt(args);
        if let Ok(text) = core::str::from_utf8(&line.bytes[..line.used]) {
            sink(text);
        }
    }
}

#[cfg(test)]
#[path = "device_log_tests.rs"]
mod tests;
