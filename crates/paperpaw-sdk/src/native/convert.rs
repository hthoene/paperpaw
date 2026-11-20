//! The conversion hook `core.convert` (`docs/abi/document.md` 4.1, #429): a
//! plugin that subscribes to it (`registration.stage`, at most four on a
//! device) converts an element a format hands on instead of reading it itself
//! (MathML in an EPUB). The format writes the element into its block as a
//! *foreign element* with the text it stands for without a hook; the host asks
//! the subscribers, in stage order, and puts the first answer's text where
//! that text was. It sees the element's markup only: no book, no card.
//!
//! In the plugin's `event`, for `Event::StageCall { stage: STAGE, payload, .. }`:
//! read the question with [`Asked::decode`] and write the answer with
//! [`Converted::encode`] (through `Reply::write_with`). Writing no answer, or
//! [`Converted::pass`], leaves the element to the next subscriber (and in the
//! end to the format's own text).
use crate::native::{Error, stage_schemas::core_convert_v1 as stage};

/// The stage's key, as `registration.stage` and `Event::StageCall` name it.
pub const STAGE: &str = stage::KEY;
/// The longest markup of one element (bytes): a longer element is not handed on.
pub const MARKUP_MAX: usize = stage::CONV_ASK_MARKUP_MAX_BYTES;
/// The longest text an answer holds (bytes).
pub const TEXT_MAX: usize = stage::CONV_TEXT_TEXT_MAX_BYTES;

/// What the host asks about one element (`conv.ask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Asked<'a> {
    element: &'a str,
    markup: &'a str,
}

impl<'a> Asked<'a> {
    /// Read the question from the payload of the stage call.
    ///
    /// # Errors
    /// `Invalid` for a payload that is not one `conv.ask`.
    pub fn decode(payload: &'a [u8]) -> Result<Self, Error> {
        let mut rows = stage::input_decode(payload).map_err(|_| Error::Invalid)?;
        let Some(Ok(stage::Record::ConvAsk(ask))) = rows.next() else {
            return Err(Error::Invalid);
        };
        Ok(Self {
            element: ask.element,
            markup: ask.markup,
        })
    }

    /// The element's local name as the format read it (`math`).
    #[must_use]
    pub const fn element(&self) -> &'a str {
        self.element
    }

    /// The element as markup: its start tag with the attributes that matter
    /// (no namespace declarations), its content, its end tag; text with
    /// `&amp;`, `&lt;` and `&gt;` escaped.
    #[must_use]
    pub const fn markup(&self) -> &'a str {
        self.markup
    }
}

/// The answer of a subscriber (`conv.text`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Converted<'a> {
    text: &'a str,
}

impl<'a> Converted<'a> {
    /// Nothing to say: the element stays as the format wrote it and the next
    /// subscriber is asked.
    #[must_use]
    pub const fn pass() -> Self {
        Self { text: "" }
    }

    /// The text the element stands for: `\n` breaks a line. Text longer than
    /// [`TEXT_MAX`], or that makes the block too long for the host to pass
    /// on, is not used: the format's own text stays.
    #[must_use]
    pub const fn text(text: &'a str) -> Self {
        Self { text }
    }

    /// Write the answer into `out` (the stage reply); the bytes written.
    ///
    /// # Errors
    /// `NoSpace` where `out` is too small (2,112 bytes hold every answer).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        stage::reply_encode(
            &[stage::Record::ConvText(stage::ConvText::new(self.text, 0))],
            out,
        )
        .map_err(Error::from_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_read_and_an_answer_written_as_the_stage_says() {
        let ask = stage::Record::ConvAsk(stage::ConvAsk::new("math", 0, "<math><mi>x</mi></math>"));
        let mut payload = [0_u8; 96];
        let n = stage::input_encode(&[ask], &mut payload).unwrap();
        let asked = Asked::decode(&payload[..n]).unwrap();
        assert_eq!(asked.element(), "math");
        assert_eq!(asked.markup(), "<math><mi>x</mi></math>");
        assert_eq!(Asked::decode(&[]), Err(Error::Invalid));

        let mut out = [0_u8; 64];
        let n = Converted::text("a/b").encode(&mut out).unwrap();
        let mut rows = stage::reply_decode(&out[..n]).unwrap();
        assert!(matches!(
            rows.next(),
            Some(Ok(stage::Record::ConvText(t))) if t.text == "a/b"
        ));
        let n = Converted::pass().encode(&mut out).unwrap();
        let mut rows = stage::reply_decode(&out[..n]).unwrap();
        assert!(matches!(
            rows.next(),
            Some(Ok(stage::Record::ConvText(t))) if t.text.is_empty()
        ));
        assert_eq!(Converted::pass().encode(&mut [0; 2]), Err(Error::NoSpace));
    }
}
