//! The picture stage `core.picture` (`docs/abi/document.md` 5): a plugin that
//! subscribes to it (`registration.stage`, at most two on a device) is asked
//! once for each picture the host prepares, before the picture is shown, and
//! may answer how it is drawn: a tone curve of 256 grays, and black and white
//! dots or the panel's grays. It sees no pixels, only where the picture is
//! shown, its size and a histogram of its grays.
//!
//! In the plugin's `event`, for `Event::StageCall { stage: STAGE, payload, .. }`:
//! read the question with [`Asked::decode`] and write the answer with
//! [`Tone::encode`] (through `Reply::write_with`). Writing no answer, or
//! [`Tone::keep`], leaves the picture as it is and lets the next subscriber
//! answer.
use super::TONE_BYTES;
use crate::native::{Error, stage_schemas::core_picture_v1 as stage};

/// The stage's key, as `registration.stage` and `Event::StageCall` name it.
pub const STAGE: &str = stage::KEY;
/// The gray bins of [`Asked::histogram`].
pub const HISTOGRAM_BINS: usize = 16;

/// Where a picture is shown (`picture.ask.place`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Place {
    /// A picture on a page of a book.
    ReaderPage,
    /// A page of a comic: a picture of a book whose format reads whole pages.
    ComicPage,
    /// A book's cover.
    Cover,
    /// A small picture of a list or a grid: a cover or a plugin's own
    /// picture no longer than a quarter of the screen's long side.
    Thumbnail,
    /// A picture of the sleep screen: a cover or a picture its plugin
    /// prepared while its screen is shown.
    SleepScreen,
    /// Another picture a plugin prepared for its own screen or widget.
    Ui,
    /// A place a newer host added.
    Other,
}

/// What the host asks about one picture (`picture.ask`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Asked<'a> {
    place: Place,
    width: u32,
    height: u32,
    flags: u32,
    histogram: [u8; HISTOGRAM_BINS],
    path: &'a str,
}

impl<'a> Asked<'a> {
    /// Read the question from the payload of the stage call.
    ///
    /// # Errors
    /// `Invalid` for a payload that is not one `picture.ask`.
    pub fn decode(payload: &'a [u8]) -> Result<Self, Error> {
        let mut rows = stage::input_decode(payload).map_err(|_| Error::Invalid)?;
        let Some(Ok(stage::Record::PictureAsk(ask))) = rows.next() else {
            return Err(Error::Invalid);
        };
        let mut histogram = [0; HISTOGRAM_BINS];
        // A shorter histogram leaves its last bins empty; a longer one is cut.
        for (bin, value) in histogram.iter_mut().zip(ask.histogram) {
            *bin = *value;
        }
        Ok(Self {
            place: match ask.place {
                0 => Place::ReaderPage,
                1 => Place::ComicPage,
                2 => Place::Cover,
                3 => Place::Thumbnail,
                4 => Place::SleepScreen,
                5 => Place::Ui,
                _ => Place::Other,
            },
            width: ask.width,
            height: ask.height,
            flags: ask.flags,
            histogram,
            path: ask.path,
        })
    }

    /// Where the picture is shown, as the host can tell (`docs/abi/document.md` 5).
    #[must_use]
    pub const fn place(&self) -> Place {
        self.place
    }

    /// The picture's width in pixels, as the host keeps it.
    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// The picture's height in pixels, as the host keeps it.
    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Whether the picture is kept in colour (the histogram is its luma).
    #[must_use]
    pub const fn colour(&self) -> bool {
        self.flags & 1 != 0
    }

    /// Whether the picture has transparency (a curve maps its gray only).
    #[must_use]
    pub const fn see_through(&self) -> bool {
        self.flags & 2 != 0
    }

    /// How many pixels are how bright: 16 bins from black (grays 0 to 15) to
    /// white (240 to 255), each 0 to 255 of the fullest bin, counted over
    /// rows spread over the picture.
    #[must_use]
    pub const fn histogram(&self) -> &[u8; HISTOGRAM_BINS] {
        &self.histogram
    }

    /// The book or file the picture is from, in the library (root 1); empty
    /// where the host names none.
    #[must_use]
    pub const fn path(&self) -> &'a str {
        self.path
    }
}

/// The answer of a subscriber (`picture.tone`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Tone<'a> {
    curve: Option<&'a [u8; TONE_BYTES]>,
    flags: u32,
}

impl<'a> Tone<'a> {
    /// Nothing to say: the picture stays as it is and the next subscriber is
    /// asked.
    #[must_use]
    pub const fn keep() -> Self {
        Self {
            curve: None,
            flags: 0,
        }
    }

    /// Every gray `v` of the picture is drawn as `curve[v]` (before the
    /// user's picture settings and the dither).
    #[must_use]
    pub const fn curve(curve: &'a [u8; TONE_BYTES]) -> Self {
        Self {
            curve: Some(curve),
            flags: 0,
        }
    }

    /// Draw the picture in black and white dots, also on a panel with grays,
    /// where the plugin that draws it names neither. Replaces
    /// [`grays`](Self::grays).
    #[must_use]
    pub const fn dots(mut self) -> Self {
        self.flags = 1;
        self
    }

    /// Draw the picture in the panel's grays where the plugin that draws it
    /// names neither. Replaces [`dots`](Self::dots).
    #[must_use]
    pub const fn grays(mut self) -> Self {
        self.flags = 2;
        self
    }

    /// Write the answer into `out` (the stage reply); the bytes written.
    ///
    /// # Errors
    /// `NoSpace` where `out` is too small (300 bytes hold every answer).
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, Error> {
        let tone = stage::PictureTone::new(self.curve.map_or(&[], |curve| &curve[..]), self.flags);
        stage::reply_encode(&[stage::Record::PictureTone(tone)], out).map_err(Error::from_status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_question_is_read_and_an_answer_written_as_the_stage_says() {
        let histogram = [9_u8; 16];
        let ask = stage::Record::PictureAsk(stage::PictureAsk::new(
            1,
            480,
            800,
            1,
            &histogram,
            "Comics/a.cbz",
        ));
        let mut payload = [0_u8; 96];
        let n = stage::input_encode(&[ask], &mut payload).unwrap();
        let asked = Asked::decode(&payload[..n]).unwrap();
        assert_eq!(asked.place(), Place::ComicPage);
        assert_eq!((asked.width(), asked.height()), (480, 800));
        assert!(asked.colour() && !asked.see_through());
        assert_eq!(asked.histogram(), &histogram);
        assert_eq!(asked.path(), "Comics/a.cbz");
        assert_eq!(Asked::decode(&[]), Err(Error::Invalid));

        let mut curve = [0_u8; TONE_BYTES];
        curve[200] = 7;
        let mut out = [0_u8; 300];
        let n = Tone::curve(&curve).dots().grays().encode(&mut out).unwrap();
        let mut rows = stage::reply_decode(&out[..n]).unwrap();
        let Some(Ok(stage::Record::PictureTone(tone))) = rows.next() else {
            panic!("no picture.tone");
        };
        assert_eq!((tone.tone.len(), tone.tone[200], tone.flags), (256, 7, 2));
        let n = Tone::keep().encode(&mut out).unwrap();
        let mut rows = stage::reply_decode(&out[..n]).unwrap();
        assert!(matches!(
            rows.next(),
            Some(Ok(stage::Record::PictureTone(tone))) if tone.tone.is_empty() && tone.flags == 0
        ));
        assert_eq!(Tone::keep().encode(&mut [0; 2]), Err(Error::NoSpace));
    }
}
