//! A page's pictures follow its text (#252): the first frame of a page draws
//! the text and keeps each picture's place empty, exactly its size, so the
//! page shows fast; the frame after draws the pictures in those places and
//! nothing moves. The pictures' boxes are asked for when the page changes
//! (a turn, a jump, a link, the book shown) and again when the screen is
//! shown while they wait (the host drops a follow-up of a screen that left
//! the top). A page laid out only in its draw (a changed setting, a turned
//! screen) and a page without pictures have one frame; a page turned before
//! its pictures were drawn never draws them.
use super::{Open, Setup, redraw};
use crate::native_block::picture_path;
use crate::native_draw::{Pictures, picture_boxes, picture_of};
use alloc::string::String;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::FrameRect,
    commands::{CommandSender, RedrawMode},
    document::Documents,
    image::{ImageId, ImageInfo},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};

/// Where the page on show is in its two frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Pass {
    /// The pictures are drawn (or the page has none).
    Whole,
    /// The page has pictures and the host draws their boxes again in a
    /// follow-up frame: every draw before it (each band of a frame, the X4's
    /// second send) keeps their places empty.
    Pending,
    /// A book just opened on a page with pictures: their boxes are asked for
    /// when the screen is shown; until then its draws keep their places empty.
    Ask,
}

impl Pass {
    /// What a draw puts in the pictures' places: while they are pending,
    /// nothing, except in the host's follow-up of their boxes (`follow_up`,
    /// each of its bands), which draws them; every draw after that does too.
    pub(super) fn drawing(&mut self, follow_up: bool) -> Pictures {
        match (*self, follow_up) {
            (Self::Pending | Self::Ask, false) => Pictures::Places,
            (Self::Pending | Self::Ask, true) | (Self::Whole, _) => {
                *self = Self::Whole;
                Pictures::Drawn
            }
        }
    }
}

/// Whether a draw is of the host's follow-up frame of the areas asked for
/// (`queue_redraw_area`): bit 0 of a screen draw's id (`screens-input.md`).
pub(super) const fn follow_up(frame: &paperpaw_sdk::native::callbacks::DrawFrame<'_>) -> bool {
    frame.id & 1 == 1
}

impl Open {
    /// Whether the page on show (laid out) has pictures.
    pub(super) fn has_pictures(&self) -> bool {
        self.page
            .as_ref()
            .is_some_and(|page| !page.images.is_empty())
    }

    /// Whether the pictures of the page on show wait for the host's
    /// follow-up frame, or to be asked for.
    pub(super) fn pictures_pending(&self) -> bool {
        self.pictures != Pass::Whole
    }

    /// The page on show is drawn again in `mode` (it was laid out in the
    /// callback: `announce`). Where it has pictures and the host takes their
    /// boxes to draw again in a frame of their own after, the page is drawn
    /// first without them (#252); else with them.
    ///
    /// # Errors
    /// The host's refusal to redraw.
    pub(super) fn redraw_page(
        &mut self,
        host: &Host<'_>,
        docs: &Documents,
        mode: RedrawMode,
    ) -> Result<(), Error> {
        // Drawn whole unless the boxes are taken below: a refused redraw
        // never leaves the pictures' places empty.
        self.pictures = Pass::Whole;
        redraw(host, mode)?;
        if !self.has_pictures() {
            return Ok(());
        }
        self.ask_missing(host, docs);
        let Some(page) = self.page.as_ref() else {
            return Ok(());
        };
        let Some(area) = Setup::read(host).ok().and_then(|setup| self.area(&setup)) else {
            return Ok(());
        };
        // The boxes from the top left of the screen's region, as its draw
        // has them (`draw_with`: the content box, then the picture).
        let origin = (f32::from(area.x), f32::from(area.y));
        if ask_second_pass(host, picture_boxes(page, origin)) {
            self.pictures = Pass::Pending;
        }
        Ok(())
    }
}

impl Open {
    /// The pictures of the page on show without a live image id (none was
    /// free when the page was laid out, or the id ended since) are asked for
    /// again by the format's index (op 16), at most [`MAX_ASKED`] a page; one
    /// the host cannot give now keeps its box (#252 P2). The ids handed out
    /// stay until they are drawn, in the frames this callback asks for.
    /// That protection ends at the next frame drawn without the id: here the
    /// text-only frame, so a new id is unprotected until the follow-up. Safe
    /// while nothing else in the reader asks for ids in between; if that
    /// changes, the follow-up meets `NotFound` and the box stays empty until
    /// the next layout.
    fn ask_missing(&mut self, host: &Host<'_>, docs: &Documents) {
        let id = self.id;
        let Some(page) = self.page.as_mut() else {
            return;
        };
        let mut out = None;
        for image in page.images.iter_mut().take(MAX_ASKED) {
            let Some((handle, Some(index))) = picture_of(&image.image.path) else {
                continue;
            };
            if live(host, handle) {
                continue;
            }
            // The reply's room only where a picture is asked for.
            if out.is_none() {
                match buffer(SERVICE_BUFFER) {
                    Ok(made) => out = Some(made),
                    Err(_) => return,
                }
            }
            let Some(out) = out.as_mut() else {
                return;
            };
            let mut input = [0; 64];
            let answer = docs.ask_image(host, id, index, 0, &mut input, out);
            if !take_answer(&mut image.image.path, index, answer.map(|p| p.image())) {
                return;
            }
        }
    }
}

/// Takes the host's answer for the picture `index` of the page into its
/// `path`: a new id is drawn; a picture the host could not give (id 0, also
/// "not decoded yet" again, or a refusal) keeps its box and is not asked for
/// again while this page is laid out (its path loses the index), so asking
/// never loops. False where the next pictures wait for the next page laid
/// out (Busy: all ids are out and none drawn yet).
fn take_answer(path: &mut String, index: u32, answer: Result<u32, Error>) -> bool {
    match answer {
        Ok(image @ 1..) => {
            if let Ok(made) = picture_path(image, index) {
                *path = made;
            }
            true
        }
        Err(Error::Busy) => false,
        Ok(0) | Err(_) => {
            // No allocation: the path held `handle:index`.
            path.clear();
            path.push('0');
            true
        }
    }
}

/// Most pictures of one page asked for again (the host's image ids).
const MAX_ASKED: usize = 8;

/// Whether `handle` names a picture the host still has.
fn live(host: &Host<'_>, handle: u32) -> bool {
    ImageId::new(handle)
        .is_ok_and(|image| !matches!(ImageInfo::read(host, image), Err(Error::NotFound)))
}

/// Asks the host to draw the pictures' boxes (`(x, y, width, height)` from
/// the top left of the screen's region) again in a frame of their own
/// right after the one asked for, with a partial refresh of the boxes only
/// (#252). Whether it takes every box.
fn ask_second_pass(host: &Host<'_>, mut boxes: impl Iterator<Item = (i32, i32, u32, u32)>) -> bool {
    let mut scratch = [0; 64];
    let mut sender = CommandSender::new(host, &mut scratch);
    // The host joins boxes past its eighth into the last; a refusal (an old
    // host, no room) draws the pictures with the text, where the boxes
    // already asked for are drawn again unchanged.
    boxes.all(|(x, y, width, height)| {
        sender
            .queue_redraw_area(RedrawMode::Partial, FrameRect::new(x, y, width, height))
            .is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::{Pass, take_answer};
    use crate::native_draw::{Pictures, picture_of};
    use alloc::string::String;
    use paperpaw_sdk::native::Error;

    #[test]
    fn a_picture_given_again_takes_its_new_id_and_keeps_its_index() {
        let mut path = String::from("0:4");
        assert!(take_answer(&mut path, 4, Ok(9)));
        assert_eq!(picture_of(&path), Some((9, Some(4))));
    }

    #[test]
    fn a_picture_the_host_cannot_give_is_not_asked_for_again() {
        // "Not decoded yet" again (id 0), a missing index, any refusal.
        for answer in [Ok(0), Err(Error::NotFound), Err(Error::Invalid)] {
            let mut path = String::from("0:4");
            assert!(take_answer(&mut path, 4, answer));
            assert_eq!(picture_of(&path), Some((0, None)));
        }
    }

    #[test]
    fn with_every_id_out_the_next_pictures_wait_for_the_next_layout() {
        let mut path = String::from("0:4");
        assert!(!take_answer(&mut path, 4, Err(Error::Busy)));
        assert_eq!(picture_of(&path), Some((0, Some(4))));
    }

    #[test]
    fn the_text_is_drawn_first_and_the_pictures_in_the_follow_up() {
        let mut pass = Pass::Pending;
        // Every band of the first frame (and its second send): no pictures.
        for _ in 0..5 {
            assert_eq!(pass.drawing(false), Pictures::Places);
        }
        assert_eq!(pass, Pass::Pending);
        // The follow-up of the boxes, each of its bands: the pictures.
        assert_eq!(pass.drawing(true), Pictures::Drawn);
        assert_eq!(pass.drawing(true), Pictures::Drawn);
        // Any frame after: with them.
        assert_eq!(pass.drawing(false), Pictures::Drawn);
        assert_eq!(pass, Pass::Whole);
    }

    #[test]
    fn a_book_opened_on_a_picture_keeps_its_place_empty_until_it_is_asked_for() {
        let mut pass = Pass::Ask;
        assert_eq!(pass.drawing(false), Pictures::Places);
        assert_eq!(pass, Pass::Ask);
    }
}
