//! `core.ui` ops 18 `clipboard_write` and 19 `clipboard_read` (`docs/abi/ui.md`
//! 7): the kernel's one clipboard, a host text with the fixed id 0 and the
//! rules of a hand-over's text (3.1): append-only parts, offset 0 starts over,
//! `last` commits.
//!
//! - **One clipboard.** The text lives in the kernel's [`ClipboardCell`] on
//!   every device (as interface 6's `Command::SetClipboard` kept it): a
//!   committed write replaces it, op 19 and interface 6 read the same text, and
//!   the commit asks the kernel pump to copy it out (`Io::Copy`, so the
//!   system clipboard where a shell has one). The store binds the cell when the
//!   dialog pump first runs; before that both ops are `ERR_STATE`.
//! - **Size.** Text only, at most 4,096 bytes ([`CLIPBOARD_MAX`], interface 6
//!   `MAX_CLIPBOARD`); a part is at most the text part size. A longer text is
//!   `ERR_NO_SPACE`, never cut. Not kept across restarts.
//! - **Write.** Any plugin, inside a callback. The new text is held apart
//!   from the old one: reads see the old clipboard until `last` commits the
//!   new (an empty committed text clears it). An uncommitted new text is
//!   dropped when the writer's callback is over: the next write by another
//!   callback, or the dialog pump between callbacks, frees it. Allocation is
//!   fallible: `ERR_NO_MEMORY`, the old clipboard stays.
//! - **Read.** Any plugin, at once, in parts; `total` 0 is empty.
//! - **Secret.** The host cannot tell a secret answer from other text:
//!   keeping it off the clipboard is the dialog plugin's duty (decision 44).
//!
//! Memory: the committed text and, while a replacement is written, the new one
//! (4 KiB each at most); the commit moves the new string in, no copy. There is
//! one staged text for the whole device, not one per plugin: a second writer's
//! callback drops the first's, so what a plugin can make the host hold for it
//! is bounded by that one 4 KiB buffer and no plugin's own heap budget is
//! involved (the text is host memory, as the committed clipboard is).
use super::{
    codec,
    host_services::Caller,
    registrations::{self as wire, Record},
    ui_service::UiServices,
    ui_store::{PART_MAX, Stamp, UiStore},
};
use crate::abi7::context::Frame;
use alloc::{rc::Rc, string::String};
use core::cell::{Cell, RefCell};
use paperpaw_native::host_glue::{ERR_INVALID, ERR_NO_MEMORY, ERR_NO_SPACE, ERR_STATE};

/// The most bytes the clipboard holds (interface 6 `MAX_CLIPBOARD`).
pub(super) const CLIPBOARD_MAX: usize = 4_096;
/// `ui_text.flags` bit 0.
const LAST: u32 = 1;

/// The kernel's clipboard text, shared by interface 6's commands and `core.ui`.
#[derive(Debug, Default)]
pub(crate) struct ClipboardCell {
    text: RefCell<String>,
    /// A commit of op 18 is yet to be copied out (`Io::Copy`).
    copy: Cell<bool>,
}

impl ClipboardCell {
    /// Interface 6: replaces the text.
    pub(crate) fn set(&self, text: String) {
        *self.text.borrow_mut() = text;
    }
    /// Interface 6: a copy of the text.
    pub(crate) fn get(&self) -> String {
        self.text.borrow().clone()
    }
    pub(crate) fn is_empty(&self) -> bool {
        self.text.borrow().is_empty()
    }
    /// A committed op 18 write waits to be copied out: yes once.
    pub(crate) fn take_copy(&self) -> bool {
        self.copy.replace(false)
    }
    /// The text for the copy, fallibly (`None`: no memory, the copy is skipped).
    pub(crate) fn text_for_copy(&self) -> Option<String> {
        let text = self.text.borrow();
        let mut copy = String::new();
        copy.try_reserve_exact(text.len()).ok()?;
        copy.push_str(&text);
        Some(copy)
    }
}

/// What the store keeps of the clipboard.
#[derive(Default)]
pub(super) struct ClipboardState {
    board: Option<Rc<ClipboardCell>>,
    /// The text being written, and the callback writing it.
    staged: String,
    by: Option<Stamp>,
}

impl ClipboardState {
    fn drop_staged(&mut self) {
        self.staged = String::new();
        self.by = None;
    }
}

impl UiStore {
    /// Binds the kernel's clipboard (once; the same cell again changes nothing).
    pub(crate) fn bind_clipboard(&self, board: &Rc<ClipboardCell>) {
        if let Ok(mut inner) = self.inner.try_borrow_mut()
            && inner.clipboard.board.is_none()
        {
            inner.clipboard.board = Some(Rc::clone(board));
        }
    }

    /// Op 18: `data` at `offset` of the new text, written by `who`'s callback;
    /// `last` commits it as the clipboard.
    pub(crate) fn clipboard_write(
        &self,
        who: Stamp,
        offset: usize,
        data: &str,
        last: bool,
    ) -> Result<(), i32> {
        let mut inner = self.inner()?;
        let state = &mut inner.clipboard;
        let board = state.board.clone().ok_or(ERR_STATE)?;
        // A text another callback left begun is dead: its writer returned.
        if state.by != Some(who) {
            state.drop_staged();
        }
        if offset == 0 {
            state.staged.clear();
        } else if offset != state.staged.len() {
            return Err(ERR_INVALID);
        }
        if state.staged.len() + data.len() > CLIPBOARD_MAX {
            return Err(ERR_NO_SPACE);
        }
        // Room is asked for when the part does not fit, and then for double
        // what is held (never past the cap): a text written in many parts
        // reallocates a few times, not once a part. Fallible; a refusal leaves
        // the text as it was.
        let need = state.staged.len() + data.len();
        if need > state.staged.capacity() {
            let want = need.max((state.staged.capacity() * 2).min(CLIPBOARD_MAX));
            state
                .staged
                .try_reserve_exact(want - state.staged.len())
                .map_err(|_| ERR_NO_MEMORY)?;
        }
        state.staged.push_str(data);
        state.by = Some(who);
        if last {
            // The new text moves in; the old one is freed.
            let text = core::mem::take(&mut state.staged);
            state.by = None;
            *board.text.borrow_mut() = text;
            board.copy.set(true);
        }
        Ok(())
    }

    /// Op 19: the part of the clipboard from `offset`, at most `max` bytes, cut
    /// at a UTF-8 boundary, handed to `with` as (total, part) straight from
    /// the text (no copy).
    pub(crate) fn clipboard_read<R>(
        &self,
        offset: usize,
        max: usize,
        with: impl FnOnce(usize, &str) -> R,
    ) -> Result<R, i32> {
        let inner = self.inner.try_borrow().map_err(|_| ERR_STATE)?;
        let board = inner.clipboard.board.as_ref().ok_or(ERR_STATE)?;
        let text = board.text.borrow();
        let total = text.len();
        if offset > total {
            return Err(ERR_INVALID);
        }
        let mut end = offset.saturating_add(max).min(total);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == offset && offset < total {
            return Err(ERR_NO_SPACE);
        }
        Ok(with(total, &text[offset..end]))
    }

    /// Between callbacks: a new text still begun is dropped (its writer's
    /// callback returned).
    pub(crate) fn clipboard_callbacks_over(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.clipboard.drop_staged();
        }
    }
}

/// Op 18.
pub(super) fn write(
    frame: &dyn Frame,
    ui: &UiServices,
    who: &Caller<'_>,
    input: &[u8],
) -> Result<(), i32> {
    let part: wire::ServiceUiText<'_> = codec::single(input)?;
    if part.text != 0 || part.flags & !LAST != 0 || part.data.len() > PART_MAX {
        return Err(ERR_INVALID);
    }
    let offset = usize::try_from(part.offset).map_err(|_| ERR_INVALID)?;
    let stamp = Stamp::new(who.owner, frame.callback_epoch()?)?;
    ui.store
        .clipboard_write(stamp, offset, part.data, part.flags & LAST != 0)
}

/// Op 19.
pub(super) fn read(
    frame: &dyn Frame,
    ui: &UiServices,
    input: &[u8],
    out: &mut [u8],
) -> Result<usize, i32> {
    let at: wire::ServiceUiTextAt = codec::single(input)?;
    if at.text != 0 {
        return Err(ERR_INVALID);
    }
    frame.callback_epoch()?;
    let offset = usize::try_from(at.offset).map_err(|_| ERR_INVALID)?;
    ui.store.clipboard_read(offset, PART_MAX, |total, part| {
        codec::encode_one(
            &Record::ServiceUiText(wire::ServiceUiText {
                text: 0,
                offset: at.offset,
                total: u32::try_from(total).map_err(|_| ERR_INVALID)?,
                flags: u32::from(offset + part.len() >= total),
                data: part,
            }),
            out,
        )
    })?
}
