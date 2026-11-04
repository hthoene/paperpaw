//! The picture behind the page, made once and drawn from a file.
//!
//! The chosen picture (JPEG, PNG or GIF) is decoded a few rows at a time
//! into a gray file the size the picture is to have, then set into a
//! screen-sized gray file (the *baked* picture) as the fit, the place and
//! the zoom say, paler by the lightening: cut, framed in white, stretched
//! or tiled. Nothing but a few rows is ever in memory; the home screen
//! draws the baked file row by row ([`paperpaw_sdk::ui::draw_image`], which
//! dithers on panels of few grays, the same pixels in every band). The
//! baked file stays until the picture, its settings or the screen's size
//! change; a note beside it ([`key`]) says what it was made of.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::image::{Fit, GrayFile, GrayWriter, ImageError, ImageSink, dimensions};

use crate::orient::{Turn, jpeg_orientation, jpeg_size, transposed};
use paperpaw_sdk::{Canvas, Env, Rect};

use crate::settings;

/// The baked picture, and the note of what it was made of.
const BAKED: &str = "background.gray";
const NOTE: &str = "background.txt";
/// The picture decoded for its size, before it is set into the screen.
const TEMP: &str = "background.part";
/// The decoded picture turned on its side (a phone held upright).
const TURNED: &str = "background.turn";

/// The most pixels a picture has on a side once decoded: more would be
/// time and card for nothing the panel can show.
const LARGEST: u32 = 2400;

/// How the picture fills the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fitting {
    /// Covers the screen, its edges cut.
    Fill,
    /// Whole, with white round it.
    Whole,
    /// Pulled to the screen's proportions.
    Stretch,
    /// As large as it is.
    Original,
    /// Repeated.
    Tile,
}

impl Fitting {
    pub(crate) fn of(value: &str) -> Self {
        match settings::FITS.iter().position(|f| *f == value) {
            Some(1) => Fitting::Whole,
            Some(2) => Fitting::Stretch,
            Some(3) => Fitting::Original,
            Some(4) => Fitting::Tile,
            _ => Fitting::Fill,
        }
    }
}

/// How the picture is rendered (see [`settings::RENDERS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Render {
    /// As the built-in home screen.
    Automatic,
    /// Every gray the panel shows, always.
    Grays,
    /// Dots of black and white, always.
    Dithered,
}

impl Render {
    pub(crate) fn of(value: &str) -> Self {
        match settings::RENDERS.iter().position(|r| *r == value) {
            Some(1) => Render::Grays,
            Some(2) => Render::Dithered,
            _ => Render::Automatic,
        }
    }
}

/// The 4 by 4 pattern of the thresholds of an ordered dither.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// `value` as black or white by the ordered dither's pattern at (`x`, `y`):
/// a gray comes as the share of white dots it asks for.
pub(crate) fn dotted(value: u8, x: usize, y: usize) -> u8 {
    let threshold = u16::from(BAYER[y % 4][x % 4]) * 16 + 8;
    if u16::from(value) > threshold { 255 } else { 0 }
}

/// What the baked picture is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Recipe {
    pub(crate) path: String,
    /// The picture file's size, so that another with the name is noticed.
    pub(crate) bytes: u64,
    /// When it was last changed, where the card tells: a picture of the
    /// same size put in its place is noticed too.
    pub(crate) modified: i64,
    pub(crate) fitting: Fitting,
    /// Where it sits: 0 left or top, 1 middle, 2 right or bottom.
    pub(crate) across: u8,
    pub(crate) down: u8,
    /// 100 to 300 percent.
    pub(crate) zoom: u32,
    /// How much paler, in percent.
    pub(crate) lighten: u32,
    /// Made in black and white dots, for a panel with grays too.
    pub(crate) dotted: bool,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

impl Recipe {
    /// What the settings ask for on a screen `width` by `height`, if a
    /// picture is chosen.
    pub(crate) fn of(env: &Env<'_>, width: u16, height: u16) -> Option<Self> {
        let path = settings::picture_path(env)?;
        let anchor = env.settings().text(settings::ANCHOR.as_str());
        let at = settings::ANCHORS
            .iter()
            .position(|a| *a == anchor)
            .unwrap_or(4);
        Some(Self {
            bytes: env.storage().size(&path).unwrap_or(0),
            modified: env.storage().modified(&path).unwrap_or(0),
            path,
            fitting: Fitting::of(env.settings().text(settings::FIT.as_str())),
            across: (at % 3) as u8,
            down: (at / 3) as u8,
            zoom: env.settings().read(&settings::ZOOM).clamp(100, 300) as u32,
            lighten: env.settings().read(&settings::LIGHTEN).clamp(0, 60) as u32,
            dotted: Render::of(env.settings().text(settings::RENDER.as_str())) == Render::Dithered,
            width,
            height,
        })
    }

    /// The note kept beside the baked picture.
    pub(crate) fn key(&self) -> String {
        // The path by its hash: a long one must not make a long note.
        format!(
            "{:016x}|{}|{}|{:?}|{}{}|{}|{}|{}|{}x{}",
            hash(&self.path),
            self.bytes,
            self.modified,
            self.fitting,
            self.across,
            self.down,
            self.zoom,
            self.lighten,
            self.dotted,
            self.width,
            self.height
        )
    }
}

/// A 64-bit FNV-1a hash of `text`.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Where the picture goes: its size on the screen, and the offset of its
/// top left corner (negative where it is cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) left: i32,
    pub(crate) top: i32,
}

/// Where a picture of `source` pixels (width, height) goes as `recipe`
/// says.
pub(crate) fn plan(recipe: &Recipe, source: (u32, u32)) -> Plan {
    let (sw, sh) = (u64::from(source.0.max(1)), u64::from(source.1.max(1)));
    let (w, h) = (u64::from(recipe.width), u64::from(recipe.height));
    let zoom = u64::from(recipe.zoom);
    // The picture's size before the zoom.
    let (mut pw, mut ph) = match recipe.fitting {
        Fitting::Fill => {
            if sw * h >= sh * w {
                (sw * h / sh, h)
            } else {
                (w, sh * w / sw)
            }
        }
        Fitting::Whole => {
            if sw * h >= sh * w {
                (w, sh * w / sw)
            } else {
                (sw * h / sh, h)
            }
        }
        Fitting::Stretch => (w, h),
        Fitting::Original | Fitting::Tile => (sw, sh),
    };
    if recipe.fitting != Fitting::Stretch {
        pw = pw * zoom / 100;
        ph = ph * zoom / 100;
    }
    // A tile is at most half the screen, so that it repeats.
    if recipe.fitting == Fitting::Tile {
        if pw > w / 2 {
            ph = ph * (w / 2).max(1) / pw.max(1);
            pw = (w / 2).max(1);
        }
        if ph > h / 2 {
            pw = pw * (h / 2).max(1) / ph.max(1);
            ph = (h / 2).max(1);
        }
    }
    let pw = pw.clamp(1, u64::from(LARGEST)) as u32;
    let ph = ph.clamp(1, u64::from(LARGEST)) as u32;
    let place = |room: i64, at: u8| room * i64::from(at) / 2;
    let left = place(i64::from(recipe.width) - i64::from(pw), recipe.across);
    let top = place(i64::from(recipe.height) - i64::from(ph), recipe.down);
    Plan {
        width: pw,
        height: ph,
        left: left as i32,
        top: top as i32,
    }
}

/// Why no picture was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Failure {
    /// The file is gone or is no picture PaperPaw reads.
    Unreadable,
    /// Not enough memory or room on the card.
    Short,
}

fn data(name: &str) -> String {
    crate::data(name)
}

/// The baked picture, if it is made for `recipe`.
pub(crate) fn baked(env: &Env<'_>, recipe: &Recipe) -> Option<ImageRef> {
    let note = paperpaw_sdk::read_file(env.storage(), &data(NOTE), 512).ok()?;
    if note != recipe.key().as_bytes() {
        return None;
    }
    let file = paperpaw_sdk::image::ImageFile::open(env.storage(), &data(BAKED))?;
    let fits = file.width == u32::from(recipe.width) && file.height == u32::from(recipe.height);
    fits.then_some(ImageRef {
        path: file.path,
        width: file.width,
        height: file.height,
    })
}

/// Forgets the baked picture where there is one (the picture was taken
/// away): the card keeps nothing of it.
pub(crate) fn forget_made(env: &Env<'_>) {
    let storage = env.storage();
    if !storage.exists(&data(BAKED)) && !storage.exists(&data(NOTE)) {
        return;
    }
    for name in [NOTE, BAKED, TEMP] {
        let _ = paperpaw_sdk::files::delete(storage, &data(name));
    }
}

/// Makes the baked picture for `recipe`, replacing the old one.
pub(crate) fn bake(env: &Env<'_>, recipe: &Recipe) -> Result<ImageRef, Failure> {
    let storage = env.storage();
    // The note goes first: a file half made is never taken for a made one.
    let _ = paperpaw_sdk::files::delete(storage, &data(NOTE));
    let result = make(env, recipe);
    let _ = paperpaw_sdk::files::delete(storage, &data(TEMP));
    let _ = paperpaw_sdk::files::delete(storage, &data(TURNED));
    match result {
        Ok(image) => {
            if storage.write(&data(NOTE), recipe.key().as_bytes()).is_err() {
                // No note: the picture could never be taken for a made one.
                let _ = paperpaw_sdk::files::delete(storage, &data(BAKED));
                return Err(Failure::Short);
            }
            Ok(image)
        }
        Err(failure) => {
            let _ = paperpaw_sdk::files::delete(storage, &data(BAKED));
            Err(failure)
        }
    }
}

fn make(env: &Env<'_>, recipe: &Recipe) -> Result<ImageRef, Failure> {
    let storage = env.storage();
    // The picture's size, from its head where that says; else it is
    // decoded as large as it is allowed and measured then.
    let mut head = alloc::vec![0u8; 16 * 1024];
    let n = storage
        .read_at(&recipe.path, 0, &mut head)
        .map_err(|_| Failure::Unreadable)?;
    head.truncate(n);
    // A phone's JPEG may keep its size after more than the head holds.
    let stored = dimensions(&head).or_else(|| jpeg_size(storage, &recipe.path));
    drop(head);
    // As the camera was held: the picture is turned upright.
    let turn = Turn::of(jpeg_orientation(storage, &recipe.path));
    let known = stored.map(|size| turn.size(size));
    let wanted = match known {
        Some(source) => plan(recipe, source),
        None => Plan {
            width: u32::from(recipe.width) * recipe.zoom / 100,
            height: u32::from(recipe.height) * recipe.zoom / 100,
            left: 0,
            top: 0,
        },
    };
    let side = wanted.width.max(wanted.height).clamp(1, LARGEST);
    let decoded = paperpaw_sdk::image::picture_from_file(
        storage,
        &recipe.path,
        &data(TEMP),
        Fit::side(side),
        false,
    )
    .ok_or(Failure::Unreadable)?;
    let decoded = if turn.transpose {
        let file = GrayFile {
            path: decoded.path.clone(),
            width: decoded.width,
            height: decoded.height,
        };
        let turned = transposed(storage, &file, &data(TURNED)).map_err(failed)?;
        ImageRef {
            path: turned.path,
            width: turned.width,
            height: turned.height,
        }
    } else {
        decoded
    };
    // The plan once the size is known, from what was decoded: that is the
    // picture's own size where it is no larger than `LARGEST`, and in its
    // proportions where it was made smaller.
    let source = match known {
        Some(source) => source,
        None => (decoded.width, decoded.height),
    };
    let plan = plan(recipe, source);
    compose(env, recipe, &plan, &decoded, turn).map_err(failed)?;
    Ok(ImageRef {
        path: data(BAKED),
        width: u32::from(recipe.width),
        height: u32::from(recipe.height),
    })
}

/// What a failure of the decode or the work on files means.
fn failed(error: ImageError) -> Failure {
    match error {
        ImageError::OutOfMemory | ImageError::Io => Failure::Short,
        _ => Failure::Unreadable,
    }
}

/// Sets the decoded picture into the baked file, row by row.
fn compose(
    env: &Env<'_>,
    recipe: &Recipe,
    plan: &Plan,
    decoded: &ImageRef,
    turn: Turn,
) -> Result<(), ImageError> {
    let storage = env.storage();
    let source = GrayFile {
        path: decoded.path.clone(),
        width: decoded.width,
        height: decoded.height,
    };
    let (width, height) = (u32::from(recipe.width), u32::from(recipe.height));
    let (sw, sh) = (source.width.max(1), source.height.max(1));
    // Rows of the decoded picture are read a few at a time, in the order
    // the screen's rows need them.
    let per = (8192 / sw as usize).max(1);
    let mut chunk: Vec<u8> = Vec::new();
    chunk
        .try_reserve_exact(per * sw as usize)
        .map_err(|_| ImageError::OutOfMemory)?;
    chunk.resize(per * sw as usize, 0);
    let (mut first, mut count) = (u32::MAX, 0u32);
    let mut out: Vec<u8> = Vec::new();
    out.try_reserve_exact(width as usize)
        .map_err(|_| ImageError::OutOfMemory)?;
    out.resize(width as usize, 255);
    let baked = data(BAKED);
    let mut writer = GrayWriter::new(storage, &baked);
    writer.start(width, height)?;
    let tile = recipe.fitting == Fitting::Tile;
    let (pw, ph) = (plan.width as i64, plan.height as i64);
    // Where the first tile starts: its edge on the side the place says.
    let (tx, ty) = if tile {
        (
            plan.left.rem_euclid(plan.width as i32),
            plan.top.rem_euclid(plan.height as i32),
        )
    } else {
        (0, 0)
    };
    for y in 0..height {
        let y = y as i64;
        let row = if tile {
            Some((y - i64::from(ty)).rem_euclid(ph))
        } else {
            let at = y - i64::from(plan.top);
            (0..ph).contains(&at).then_some(at)
        };
        match row {
            None => out.fill(255),
            Some(at) => {
                let sy = ((at * i64::from(sh) / ph) as u32).min(sh - 1);
                let sy = if turn.flip_y { sh - 1 - sy } else { sy };
                if !(first..first.saturating_add(count)).contains(&sy) {
                    // Upside down, the rows read lie before the one wanted.
                    first = if turn.flip_y {
                        (sy + 1).saturating_sub(per as u32)
                    } else {
                        sy
                    };
                    count = source.read_rows(storage, first, &mut chunk);
                    if count == 0 {
                        return Err(ImageError::Io);
                    }
                }
                let line = &chunk[(sy - first) as usize * sw as usize..][..sw as usize];
                for (x, pixel) in out.iter_mut().enumerate() {
                    let x = x as i64;
                    let column = if tile {
                        Some((x - i64::from(tx)).rem_euclid(pw))
                    } else {
                        let at = x - i64::from(plan.left);
                        (0..pw).contains(&at).then_some(at)
                    };
                    *pixel = column.map_or(255, |c| {
                        let sx = ((c * i64::from(sw) / pw) as usize).min(sw as usize - 1);
                        let sx = if turn.flip_x {
                            sw as usize - 1 - sx
                        } else {
                            sx
                        };
                        let value = lighten(line[sx], recipe.lighten);
                        if recipe.dotted {
                            dotted(value, x as usize, y as usize)
                        } else {
                            value
                        }
                    });
                }
            }
        }
        writer.row(&out)?;
    }
    writer.finish()
}

/// `value` paler by `percent`.
pub(crate) fn lighten(value: u8, percent: u32) -> u8 {
    let v = u32::from(value);
    (v + (255 - v) * percent.min(100) / 100) as u8
}

/// Draws the baked picture over the whole screen.
pub(crate) fn draw(canvas: &mut Canvas<'_>, env: &Env<'_>, image: &ImageRef) {
    let area = Rect::new(0, 0, image.width as u16, image.height as u16);
    paperpaw_sdk::ui::draw_image(canvas, env.storage(), image, area);
}

/// How bright the picture is in `area`, 0 to 255 (the mean of a few of
/// its pixels), to choose text that reads on it; `None` where there is no
/// picture.
pub(crate) fn brightness(env: &Env<'_>, image: &ImageRef, area: Rect) -> Option<u8> {
    let file = GrayFile {
        path: image.path.clone(),
        width: image.width,
        height: image.height,
    };
    let mut row = alloc::vec![0u8; file.width as usize];
    let (mut sum, mut n) = (0u64, 0u64);
    let step = (area.height / 6).max(1);
    let mut y = area.y;
    while y < area.y + area.height && u32::from(y) < file.height {
        if !file.read_row(env.storage(), u32::from(y), &mut row) {
            return None;
        }
        let x1 = (area.x + area.width).min(file.width as u16);
        for x in (area.x..x1).step_by(4) {
            sum += u64::from(row[usize::from(x)]);
            n += 1;
        }
        y += step;
    }
    (n > 0).then(|| (sum / n) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(fitting: Fitting, zoom: u32, across: u8, down: u8) -> Recipe {
        Recipe {
            path: String::from("p.png"),
            bytes: 1,
            modified: 0,
            fitting,
            across,
            down,
            zoom,
            lighten: 0,
            dotted: false,
            width: 480,
            height: 800,
        }
    }

    #[test]
    fn fill_covers_the_screen_and_cuts_the_sides_of_a_wide_picture() {
        let p = plan(&recipe(Fitting::Fill, 100, 1, 1), (1600, 1000));
        assert_eq!((p.width, p.height), (1280, 800));
        assert_eq!((p.left, p.top), (-400, 0));
    }

    #[test]
    fn fill_at_the_left_keeps_the_left_edge() {
        let p = plan(&recipe(Fitting::Fill, 100, 0, 1), (1600, 1000));
        assert_eq!(p.left, 0);
        let p = plan(&recipe(Fitting::Fill, 100, 2, 1), (1600, 1000));
        assert_eq!(p.left, -800);
    }

    #[test]
    fn whole_leaves_white_above_and_below() {
        let p = plan(&recipe(Fitting::Whole, 100, 1, 1), (1600, 1000));
        assert_eq!((p.width, p.height), (480, 300));
        assert_eq!((p.left, p.top), (0, 250));
        let at_top = plan(&recipe(Fitting::Whole, 100, 1, 0), (1600, 1000));
        assert_eq!(at_top.top, 0);
    }

    #[test]
    fn zoom_makes_it_larger_about_the_place() {
        let p = plan(&recipe(Fitting::Fill, 200, 1, 1), (800, 800));
        assert_eq!((p.width, p.height), (1600, 1600));
        assert_eq!((p.left, p.top), (-560, -400));
    }

    #[test]
    fn stretch_is_the_screen_and_ignores_the_zoom() {
        let p = plan(&recipe(Fitting::Stretch, 250, 0, 0), (123, 456));
        assert_eq!((p.width, p.height, p.left, p.top), (480, 800, 0, 0));
    }

    #[test]
    fn original_keeps_the_size_and_sits_where_told() {
        let p = plan(&recipe(Fitting::Original, 100, 2, 2), (200, 100));
        assert_eq!((p.width, p.height), (200, 100));
        assert_eq!((p.left, p.top), (280, 700));
    }

    #[test]
    fn a_tile_is_at_most_half_the_screen() {
        let p = plan(&recipe(Fitting::Tile, 100, 0, 0), (1000, 500));
        assert_eq!((p.width, p.height), (240, 120));
    }

    #[test]
    fn dots_are_black_or_white_and_follow_the_gray() {
        let white = (0..16).filter(|i| dotted(128, i % 4, i / 4) == 255).count();
        assert_eq!(white, 8);
        assert!((0..16).all(|i| dotted(255, i % 4, i / 4) == 255));
        assert!((0..16).all(|i| dotted(0, i % 4, i / 4) == 0));
    }

    #[test]
    fn the_render_is_read_from_the_choice() {
        assert_eq!(Render::of("picture-home.render.grays"), Render::Grays);
        assert_eq!(Render::of("picture-home.render.dither"), Render::Dithered);
        assert_eq!(Render::of("picture-home.render.auto"), Render::Automatic);
        assert_eq!(Render::of("nonsense"), Render::Automatic);
    }

    #[test]
    fn a_long_path_makes_a_short_key() {
        let mut long = recipe(Fitting::Fill, 100, 1, 1);
        long.path = "a".repeat(600);
        let mut other = long.clone();
        other.path = "b".repeat(600);
        assert!(long.key().len() < 120, "{}", long.key().len());
        assert_ne!(long.key(), other.key());
    }

    #[test]
    fn lightening_goes_toward_white() {
        assert_eq!(lighten(0, 0), 0);
        assert_eq!(lighten(0, 50), 127);
        assert_eq!(lighten(100, 100), 255);
        assert_eq!(lighten(255, 60), 255);
    }

    #[test]
    fn the_key_tells_every_setting() {
        let a = recipe(Fitting::Fill, 100, 1, 1);
        let mut b = a.clone();
        b.zoom = 125;
        assert_ne!(a.key(), b.key());
        let mut c = a.clone();
        c.height = 480;
        assert_ne!(a.key(), c.key());
    }
}
