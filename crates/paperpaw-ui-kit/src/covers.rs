//! Book covers as pictures the host draws: asking the library for a book's
//! cover, remembering which pictures a draw puts on the canvas after its paint
//! is replayed, and fitting a picture into a place.
//!
//! A cover is an image id the host keeps in its cover cache and may end to make
//! room, so a draw asks for it again each time; a cover that is still being made
//! is [`Covered::Busy`] and drawn at the next tick.
use alloc::vec::Vec;

use paperpaw_sdk::{
    Rect,
    native::{
        Error, Host,
        image::ImageId,
        library::{BookRef, Library},
    },
};

use crate::mem;

/// A cover the host has made, and how big it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cover {
    pub image: ImageId,
    pub width: u32,
    pub height: u32,
}

/// Where a book's cover stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Covered {
    /// Ready to draw.
    Ready(Cover),
    /// Being made: asked again at the next tick.
    Busy,
    /// None to show: the book has none, it is gone from the catalog, or the
    /// host draws no pictures.
    None,
}

/// The cover of the book `key` of catalog `generation` at the list's size.
///
/// # Errors
/// [`Error::NoMemory`]; the host's other refusals are [`Covered::None`] (a
/// book is shown without a cover), except that one that is still being made
/// is [`Covered::Busy`].
pub fn cover(
    host: &Host<'_>,
    library: &Library,
    (key, generation): (u32, u32),
) -> Result<Covered, Error> {
    let (mut input, mut out) = (mem::buffer(1024)?, mem::buffer(mem::SERVICE_BUFFER)?);
    let reference = BookRef::catalog(key, generation)?;
    match library.cover(host, reference, 0, &mut input, &mut out) {
        Ok(cover) => Ok(Covered::Ready(Cover {
            image: cover.image(),
            width: cover.info().width(),
            height: cover.info().height(),
        })),
        Err(Error::Busy) => Ok(Covered::Busy),
        Err(Error::NoMemory) => Err(Error::NoMemory),
        Err(_) => Ok(Covered::None),
    }
}

/// The pictures a draw puts on the canvas after its paint is replayed (a cover
/// goes where nothing else is drawn), and whether one is still being made.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Pictures {
    pub placed: Vec<(ImageId, Rect)>,
    pub busy: bool,
}

impl Pictures {
    /// `image` goes at `at`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn place(&mut self, image: ImageId, at: Rect) -> Result<(), Error> {
        mem::push(&mut self.placed, (image, at))
    }
}

/// `size` (width, height) fitted into `place`, as large as fits and in the shape it
/// has; `at` (x, y) says where in the place, in thousandths (0 left or top,
/// 1,000 right or bottom).
#[must_use]
pub fn fitted(size: (u32, u32), place: Rect, at: (u16, u16)) -> Rect {
    let (width, height) = (u64::from(size.0.max(1)), u64::from(size.1.max(1)));
    let (room_w, room_h) = (u64::from(place.width), u64::from(place.height));
    let (w, h) = if width.saturating_mul(room_h) > height.saturating_mul(room_w) {
        (room_w, (height.saturating_mul(room_w) / width).max(1))
    } else {
        ((width.saturating_mul(room_h) / height).max(1), room_h)
    };
    let w = u16::try_from(w.min(room_w)).unwrap_or(place.width);
    let h = u16::try_from(h.min(room_h)).unwrap_or(place.height);
    let share = |free: u16, thousandths: u16| {
        u16::try_from(u32::from(free) * u32::from(thousandths.min(1000)) / 1000).unwrap_or(free)
    };
    Rect::new(
        place.x.saturating_add(share(place.width - w, at.0)),
        place.y.saturating_add(share(place.height - h, at.1)),
        w,
        h,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_fitted_in_its_shape_where_the_place_says() {
        let place = Rect::new(10, 20, 100, 100);
        // Twice as high as wide: as high as the place, half as wide.
        assert_eq!(
            fitted((50, 100), place, (0, 500)),
            Rect::new(10, 20, 50, 100)
        );
        assert_eq!(
            fitted((50, 100), place, (500, 1000)),
            Rect::new(35, 20, 50, 100)
        );
        // Wider than high, at the bottom.
        assert_eq!(
            fitted((100, 50), place, (500, 1000)),
            Rect::new(10, 70, 100, 50)
        );
    }

    #[test]
    fn a_degenerate_place_or_picture_is_fitted_without_panic() {
        assert_eq!(fitted((0, 0), Rect::new(1, 2, 0, 0), (500, 500)).width, 0);
        let _ = fitted(
            (u32::MAX, u32::MAX),
            Rect::new(0, 0, u16::MAX, u16::MAX),
            (1000, 1000),
        );
        let _ = fitted((7, 3), Rect::new(u16::MAX, u16::MAX, 5, 5), (u16::MAX, 0));
    }
}
