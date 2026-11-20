//! A `<math>` element as the chapter keeps it (#429): the format does not
//! convert MathML itself. It writes the text the formula says without help (the
//! element's `alttext`, else the text of its leaves) and keeps the element as
//! markup in `<n>.fgn`, for the plugin that converts it (`core.convert`,
//! `docs/abi/document.md` 4.1): the host puts that plugin's text where this
//! text was. Where no plugin does, this text stays.

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::xml::{Attrs, escape, try_decode};

use super::{Done, Formula, FormulaMaker};
use crate::fallible;

/// The longest markup kept (the `doc_ruby` text holds 1,024 bytes): a longer
/// element is left to its own text.
pub(crate) const MAX_MARKUP: usize = 1000;
/// The longest text written for an element.
const MAX_TEXT: usize = 1000;
/// The deepest element kept (deeper: its own text).
const MAX_DEPTH: usize = 32;

/// The attributes of MathML the conversion reads: the element's `alttext` and
/// `display`, a fence's `open`, `close` and `separators`.
const KEPT: [&str; 5] = ["alttext", "display", "open", "close", "separators"];

/// What is kept of the element being read.
struct Capture {
    markup: String,
    /// The names of the open elements.
    open: Vec<String>,
    /// Open elements that hold no text of the formula (`annotation`).
    quiet: u32,
    text: String,
    alt: Option<String>,
    display: bool,
    /// Too long or too deep: no markup.
    over: bool,
}

impl Capture {
    fn push_open(&mut self, tag: &str, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        if self.open.len() >= MAX_DEPTH {
            self.over = true;
        }
        if !self.over {
            fallible::push_char(&mut self.markup, '<')?;
            fallible::push_str(&mut self.markup, tag)?;
            for name in KEPT {
                if let Some(value) = attrs.try_get(name).map_err(|_| fallible::OOM)? {
                    let mut escaped = String::new();
                    escape(value.trim(), &mut escaped);
                    fallible::try_write!(&mut self.markup, " {name}=\"{escaped}\"")?;
                }
            }
            fallible::push_char(&mut self.markup, '>')?;
            self.over = self.markup.len() > MAX_MARKUP;
        }
        if matches!(tag, "annotation" | "annotation-xml" | "mphantom") {
            self.quiet += 1;
        }
        fallible::push(&mut self.open, fallible::string(tag)?)
    }
}

impl Formula for Capture {
    fn start(&mut self, tag: &str, attrs: &Attrs<'_>) -> Result<(), DocumentError> {
        self.push_open(tag, attrs)
    }

    fn text(&mut self, text: &str, cdata: bool) -> Result<(), DocumentError> {
        let decoded = if cdata {
            alloc::borrow::Cow::Borrowed(text)
        } else {
            try_decode(text).map_err(|_| fallible::OOM)?
        };
        let word = decoded.trim();
        if self.quiet == 0 && self.text.len() < MAX_TEXT && !word.is_empty() {
            if !self.text.is_empty() {
                fallible::push_char(&mut self.text, ' ')?;
            }
            fallible::push_str(&mut self.text, word)?;
        }
        if self.over || text.trim().is_empty() {
            return Ok(());
        }
        // Spaces between elements mean nothing; inside a text, one is enough.
        let mut escaped = String::new();
        let mut blank = false;
        for c in decoded.trim().chars() {
            if c.is_whitespace() {
                blank = true;
                continue;
            }
            if blank {
                escaped.push(' ');
                blank = false;
            }
            escape(c.encode_utf8(&mut [0; 4]), &mut escaped);
        }
        fallible::push_str(&mut self.markup, &escaped)?;
        self.over = self.markup.len() > MAX_MARKUP;
        Ok(())
    }

    fn end(&mut self) -> Result<Option<Done>, DocumentError> {
        let Some(tag) = self.open.pop() else {
            return Ok(None);
        };
        if matches!(tag.as_str(), "annotation" | "annotation-xml" | "mphantom") {
            self.quiet = self.quiet.saturating_sub(1);
        }
        if !self.over {
            fallible::try_write!(&mut self.markup, "</{tag}>")?;
            self.over = self.markup.len() > MAX_MARKUP;
        }
        if !self.open.is_empty() {
            return Ok(None);
        }
        let text = match self.alt.take().filter(|alt| !alt.is_empty()) {
            Some(alt) => alt,
            None => core::mem::take(&mut self.text),
        };
        let mut text = text;
        let mut cut = MAX_TEXT.min(text.len());
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        Ok(Some(Done {
            text,
            display: self.display,
            markup: (!self.over).then(|| core::mem::take(&mut self.markup)),
        }))
    }
}

/// The capture of the `<math>` element with these attributes.
pub(crate) const MAKER: FormulaMaker = |attrs| {
    let alt = match attrs.try_get("alttext").map_err(|_| fallible::OOM)? {
        Some(alt) => Some(fallible::string(alt.trim())?),
        None => None,
    };
    let display = attrs
        .try_get("display")
        .map_err(|_| fallible::OOM)?
        .as_deref()
        == Some("block");
    let mut capture = Capture {
        markup: String::new(),
        open: Vec::new(),
        quiet: 0,
        text: String::new(),
        alt,
        display,
        over: false,
    };
    capture.push_open("math", attrs)?;
    let capture: Box<dyn Formula> = fallible::try_box(capture)?;
    Ok(capture)
};
