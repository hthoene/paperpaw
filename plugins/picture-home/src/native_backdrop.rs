//! The picture behind the page on interface 7, without the host: which picture
//! the settings ask for, what the host is asked to make of it, what its answer
//! leads to, and where the picture that is ready is drawn.
//!
//! The host decodes the picture in the background (`image_prepare`, one answer
//! `ImageReady` for each request) and draws it scaled into a rectangle
//! (`canvas.draw_image`). So the recipe of interface 6 is met as far as that
//! reaches: the fitting, the place and the zoom are the rectangle (the same
//! `native_picture::plan`), the paleness is the blend with the paper below it
//! (transparency), tiles are drawn one by one. What the facade cannot do is
//! named in `PORTING-7.md`: the turn of a phone's picture (EXIF), the tone
//! (brightness and contrast), picture in dots by force, stretching, and tiles
//! past [`TILES_MAX`].
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::Rect;
use paperpaw_sdk::native::{
    Error,
    image::{ImageId, ImageStatus, LUMA_BYTES, SIDE_MAX, TONE_BYTES},
};

use crate::native_picture::{Fitting, Plan, Recipe, lighten, plan, tone};

/// The first cookie of the requests of this screen (the rest count up from it).
const COOKIE: u32 = 0x5048_0000;
/// Tiles drawn at most: each is one call of the host, which reads the picture
/// again. A picture that would repeat more often is not drawn.
pub const TILES_MAX: i64 = 64;

/// The side of the host's grid of brightness (8 by 8 cells).
const GRID: i64 = 8;
/// The points of the plate read for its backdrop's brightness.
const SAMPLE_COLUMNS: u16 = 16;
const SAMPLE_ROWS: u16 = 3;

/// What became of the picture asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    /// Asked; the answer has this cookie.
    Asked(u32),
    /// Made; its id, the size actually made and the host's grid of the
    /// brightness behind it (none where the host made none).
    Held(ImageId, (u32, u32), Option<[u8; LUMA_BYTES]>),
    /// Not made, or not asked: the page has a plain background.
    Plain,
}

/// The picture of one recipe and how far it is.
#[derive(Debug)]
struct Entry {
    key: String,
    recipe: Recipe,
    slot: Slot,
}

/// What a change of the recipe leaves to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Change {
    /// A picture the host holds that is of no use now.
    pub release: Option<ImageId>,
    /// A request to send (the cookie and the longest side), for the path of
    /// [`Backdrop::path`].
    pub ask: Option<(u32, u32)>,
}

/// What an answer of the host leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landed {
    /// The picture is held and shows.
    Ready,
    /// No picture was made, and why: the page stays plain.
    Failed(Reason),
    /// An answer nobody waits for; the picture it holds, if any, is to be
    /// released.
    Ignore(Option<ImageId>),
}

/// Why no picture was made, in the words of interface 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The file is gone or is no picture PaperPaw reads.
    Unreadable,
    /// Not enough memory or room on the card.
    Short,
}

impl Reason {
    /// The message of the catalog that tells it.
    #[must_use]
    pub const fn message(self) -> &'static str {
        match self {
            Self::Unreadable => "picture-home.unreadable",
            Self::Short => "picture-home.short",
        }
    }

    const fn of(status: ImageStatus) -> Self {
        match status {
            ImageStatus::CardFailure | ImageStatus::NoMemory => Self::Short,
            _ => Self::Unreadable,
        }
    }
}

/// A place of the picture on the screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// What to draw: the picture, where, and how.
#[derive(Debug, PartialEq, Eq)]
pub struct Drawing {
    pub image: ImageId,
    /// The screen's size, which the paper is painted over first where the
    /// picture does not cover it or is paler than itself.
    pub screen: (u16, u16),
    pub paper: bool,
    /// Scale to the place and drop the picture's shape.
    pub stretch: bool,
    /// Dots or grays by force; the panel's own rule where neither.
    pub render: Rendering,
    /// 255 minus the opacity: 0 opaque.
    pub transparency: u8,
    pub places: Vec<Place>,
}

/// How the picture is drawn: by the panel's rule, or in dots or in grays by force.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rendering {
    Panel,
    Dots,
    Grays,
}

/// The picture behind the page.
#[derive(Debug, Default)]
pub struct Backdrop {
    entry: Option<Entry>,
}

impl Backdrop {
    /// Wants the picture of `recipe` (none: no picture). One that is the same as
    /// the one held, asked for or given up is left as it is; another gives the
    /// old back and is asked for; `cookie` counts the requests of the plugin.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; nothing changes then.
    pub fn want(&mut self, recipe: Option<Recipe>, cookie: &mut u32) -> Result<Change, Error> {
        let Some(recipe) = recipe else {
            return Ok(self.clear());
        };
        let key = recipe.key()?;
        if self.entry.as_ref().is_some_and(|entry| entry.key == key) {
            return Ok(Change::default());
        }
        *cookie = cookie.wrapping_add(1) & 0xffff;
        let number = COOKIE | *cookie;
        let ask = (number, side(&recipe));
        let release = self.clear().release;
        self.entry = Some(Entry {
            key,
            recipe,
            slot: Slot::Asked(number),
        });
        Ok(Change {
            release,
            ask: Some(ask),
        })
    }

    /// No picture wanted: what is held goes.
    pub fn clear(&mut self) -> Change {
        let release = match self.entry.take().map(|entry| entry.slot) {
            Some(Slot::Held(image, ..)) => Some(image),
            _ => None,
        };
        Change { release, ask: None }
    }

    /// The path to ask for (empty without a wish).
    #[must_use]
    pub fn path(&self) -> &str {
        self.entry
            .as_ref()
            .map_or("", |entry| entry.recipe.path.as_str())
    }

    /// The tone curve the request carries: `None` where the recipe leaves every
    /// gray as it is (no brightness and no contrast change).
    #[must_use]
    pub fn tone(&self) -> Option<[u8; TONE_BYTES]> {
        let recipe = &self.entry.as_ref()?.recipe;
        (recipe.brightness != 0 || recipe.contrast != 0)
            .then(|| tone(recipe.brightness, recipe.contrast))
    }

    /// How bright the held picture is behind `plate` (0 black to 255 white), from
    /// the host's grid of the prepared picture: points across the plate are
    /// read from the grid cell of the picture they fall on (what the picture
    /// does not cover is the paper, white), paler by the recipe's lightening.
    /// `None` where no picture is held or the host made no grid.
    #[must_use]
    pub fn brightness_behind(&self, plate: Rect) -> Option<u8> {
        let Some(Entry {
            recipe,
            slot: Slot::Held(_, size, Some(grid)),
            ..
        }) = self.entry.as_ref()
        else {
            return None;
        };
        let place = if recipe.fitting == Fitting::Stretch {
            Plan {
                width: u32::from(recipe.width),
                height: u32::from(recipe.height),
                left: 0,
                top: 0,
            }
        } else {
            plan(recipe, *size)
        };
        let tiled = recipe.fitting == Fitting::Tile;
        let (width, height) = (
            i64::from(place.width.max(1)),
            i64::from(place.height.max(1)),
        );
        let (mut sum, mut count) = (0u32, 0u32);
        for row in 0..SAMPLE_ROWS {
            for column in 0..SAMPLE_COLUMNS {
                let x = i64::from(plate.x)
                    + (2 * i64::from(column) + 1) * i64::from(plate.width)
                        / (2 * i64::from(SAMPLE_COLUMNS));
                let y = i64::from(plate.y)
                    + (2 * i64::from(row) + 1) * i64::from(plate.height)
                        / (2 * i64::from(SAMPLE_ROWS));
                let (mut u, mut v) = (x - i64::from(place.left), y - i64::from(place.top));
                if tiled {
                    u = u.rem_euclid(width);
                    v = v.rem_euclid(height);
                }
                let value = if (0..width).contains(&u) && (0..height).contains(&v) {
                    let cell = (v * GRID / height) * GRID + u * GRID / width;
                    grid.get(usize::try_from(cell).unwrap_or(0))
                        .copied()
                        .unwrap_or(255)
                } else {
                    255
                };
                sum += u32::from(lighten(value, recipe.lighten));
                count += 1;
            }
        }
        u8::try_from(sum / count.max(1)).ok()
    }

    /// The request could not be sent: the page stays plain.
    pub fn refused(&mut self) {
        if let Some(entry) = self.entry.as_mut() {
            entry.slot = Slot::Plain;
        }
    }

    /// The host's answer to request `cookie`.
    pub fn ready(
        &mut self,
        cookie: u32,
        status: ImageStatus,
        image: Option<ImageId>,
        size: (u32, u32),
        luma: Option<&[u8; LUMA_BYTES]>,
    ) -> Landed {
        let Some(entry) = self
            .entry
            .as_mut()
            .filter(|entry| entry.slot == Slot::Asked(cookie))
        else {
            return Landed::Ignore(image);
        };
        match (status, image) {
            (ImageStatus::Ready, Some(image)) if size.0 > 0 && size.1 > 0 => {
                entry.slot = Slot::Held(image, size, luma.copied());
                Landed::Ready
            }
            (status, image) => {
                entry.slot = Slot::Plain;
                // A picture with an answer that is no picture is given back.
                match image {
                    Some(image) if status == ImageStatus::Ready => Landed::Ignore(Some(image)),
                    _ => Landed::Failed(Reason::of(status)),
                }
            }
        }
    }

    /// Whether `cookie` is the request this screen waits for.
    #[must_use]
    pub fn waits_for(&self, cookie: u32) -> bool {
        self.entry
            .as_ref()
            .is_some_and(|entry| entry.slot == Slot::Asked(cookie))
    }

    /// Whether the picture is being made: the screen says so instead of the page.
    #[must_use]
    pub fn making(&self) -> bool {
        matches!(
            self.entry,
            Some(Entry {
                slot: Slot::Asked(_),
                ..
            })
        )
    }

    /// Whether a picture is made and held.
    #[must_use]
    pub fn held(&self) -> bool {
        matches!(
            self.entry,
            Some(Entry {
                slot: Slot::Held(..),
                ..
            })
        )
    }

    /// Where and how the held picture is drawn.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn drawing(&self) -> Result<Option<Drawing>, Error> {
        let Some(Entry {
            recipe,
            slot: Slot::Held(image, size, _),
            ..
        }) = self.entry.as_ref()
        else {
            return Ok(None);
        };
        let screen = (recipe.width, recipe.height);
        let mut places = Vec::new();
        let stretch = recipe.fitting == Fitting::Stretch;
        if stretch {
            places.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
            places.push(Place {
                x: 0,
                y: 0,
                width: u32::from(screen.0),
                height: u32::from(screen.1),
            });
        } else {
            tiles(recipe, plan(recipe, *size), &mut places)?;
        }
        if places.is_empty() {
            return Ok(None);
        }
        let covers = matches!(recipe.fitting, Fitting::Fill | Fitting::Stretch);
        let render = match (recipe.dotted, recipe.grays) {
            (true, _) => Rendering::Dots,
            (_, true) => Rendering::Grays,
            _ => Rendering::Panel,
        };
        Ok(Some(Drawing {
            image: *image,
            screen,
            paper: !covers || recipe.lighten > 0,
            stretch,
            render,
            transparency: transparency(recipe.lighten),
            places,
        }))
    }
}

/// The longest side to ask the host for: as long as the picture is drawn, a
/// picture cut to fill the screen a half more (a wide one is drawn wider than
/// the screen). The host never makes a picture larger than the file.
#[must_use]
pub fn side(recipe: &Recipe) -> u32 {
    let long = u32::from(recipe.width.max(recipe.height));
    let zoom = recipe.zoom.clamp(100, 300);
    let wanted = match recipe.fitting {
        Fitting::Fill => long * zoom / 100 * 3 / 2,
        Fitting::Whole => long * zoom / 100,
        Fitting::Stretch => long,
        Fitting::Original => SIDE_MAX,
        Fitting::Tile => long / 2,
    };
    wanted.clamp(1, SIDE_MAX)
}

/// The blend with the paper that makes a picture `percent` paler.
#[must_use]
pub fn transparency(percent: u32) -> u8 {
    u8::try_from(percent.min(100) * 255 / 100).unwrap_or(u8::MAX)
}

/// The places of the picture `plan` puts on the screen: one, or the tiles that
/// cover it (the first starting where the place says, as interface 6 made it).
fn tiles(recipe: &Recipe, plan: Plan, places: &mut Vec<Place>) -> Result<(), Error> {
    let one = |x: i32, y: i32| Place {
        x,
        y,
        width: plan.width,
        height: plan.height,
    };
    if recipe.fitting != Fitting::Tile {
        places.try_reserve_exact(1).map_err(|_| Error::NoMemory)?;
        places.push(one(plan.left, plan.top));
        return Ok(());
    }
    let (w, h) = (i64::from(recipe.width), i64::from(recipe.height));
    let (pw, ph) = (i64::from(plan.width.max(1)), i64::from(plan.height.max(1)));
    // The tile before the first edge shows its end.
    let x0 = i64::from(plan.left).rem_euclid(pw) - pw;
    let y0 = i64::from(plan.top).rem_euclid(ph) - ph;
    let across = (w - x0 + pw - 1) / pw;
    let down = (h - y0 + ph - 1) / ph;
    if across * down > TILES_MAX {
        return Ok(());
    }
    places
        .try_reserve_exact((across * down) as usize)
        .map_err(|_| Error::NoMemory)?;
    for row in 0..down {
        for column in 0..across {
            let (x, y) = (x0 + column * pw, y0 + row * ph);
            // x0 and y0 are above -pw and -ph, so these are on the screen's side
            // of what an i32 holds.
            places.push(one(x as i32, y as i32));
        }
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests;
