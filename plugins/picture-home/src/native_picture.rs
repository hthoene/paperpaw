//! What the picture home screen decides, without the host: how the chosen
//! picture is fitted, placed, toned and kept (the recipe, its note, the plan
//! and the crop that is needed of it), what a JPEG says about its size and
//! turn, which pictures a folder offers, and where the status bar sits. The
//! same rules as the interface-6 screen (`picture.rs`, `orient.rs`, `bar.rs`,
//! `pick.rs`), with fallible allocation and the facts given by the caller
//! instead of read from an environment. Decoding, drawing and the host's files
//! are the native screen's (see `PORTING-7.md`).
use alloc::{string::String, vec::Vec};
use core::fmt::Write;
use paperpaw_sdk::native::Error;

use crate::settings;

/// The most pixels a picture has on a side once decoded.
pub const LARGEST: u32 = 2400;
/// Pictures listed at most: a longer folder is cut, not slowed down.
pub const MOST: usize = 300;
/// The bytes of names kept at most (the heap is small): a folder whose
/// names are longer is cut sooner, at the first name that does not fit.
pub const NAMES: usize = 24 * 1024;

/// A text that grows by fallible allocation only.
struct Out(String);

impl Write for Out {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        self.0
            .try_reserve(text.len())
            .map_err(|_| core::fmt::Error)?;
        self.0.push_str(text);
        Ok(())
    }
}

fn copy(text: &str) -> Result<String, Error> {
    let mut out = String::new();
    out.try_reserve_exact(text.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(text);
    Ok(out)
}

/// How the picture fills the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fitting {
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
    /// The fitting a choice names (the first for any other value).
    #[must_use]
    pub fn of(value: &str) -> Self {
        match settings::FITS.iter().position(|f| *f == value) {
            Some(1) => Self::Whole,
            Some(2) => Self::Stretch,
            Some(3) => Self::Original,
            Some(4) => Self::Tile,
            _ => Self::Fill,
        }
    }
}

/// How the picture is rendered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    /// As the built-in home screen.
    Automatic,
    /// Every gray the panel shows, always.
    Grays,
    /// Dots of black and white, always.
    Dithered,
}

impl Render {
    /// The render a choice names (automatic for any other value).
    #[must_use]
    pub fn of(value: &str) -> Self {
        match settings::RENDERS.iter().position(|r| *r == value) {
            Some(1) => Self::Grays,
            Some(2) => Self::Dithered,
            _ => Self::Automatic,
        }
    }
}

/// What the settings and the card say of the picture.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    /// The picture's path on the card; empty when none is chosen.
    pub path: &'a str,
    /// The file's size, so that another with the name is noticed.
    pub bytes: u64,
    /// When it was last changed, where the card tells.
    pub modified: i64,
    pub fit: &'a str,
    pub anchor: &'a str,
    pub zoom: i64,
    pub lighten: i64,
    pub brightness: &'a str,
    pub contrast: &'a str,
    pub render: &'a str,
}

/// What the baked picture is made of.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipe {
    pub path: String,
    pub bytes: u64,
    pub modified: i64,
    pub fitting: Fitting,
    /// Where it sits: 0 left or top, 1 middle, 2 right or bottom.
    pub across: u8,
    pub down: u8,
    /// 100 to 300 percent.
    pub zoom: u32,
    /// How much paler, in percent.
    pub lighten: u32,
    /// From -2 (much darker) to 2 (much lighter), and from -1 (lower) to 1
    /// (higher) contrast.
    pub brightness: i8,
    pub contrast: i8,
    /// Made in black and white dots, for a panel with grays too.
    pub dotted: bool,
    /// Made in every gray the panel shows, always.
    pub grays: bool,
    pub width: u16,
    pub height: u16,
}

/// The step of a choice from its middle: -2 to 2.
fn step(value: &str, values: &[&str], middle: usize) -> i8 {
    values
        .iter()
        .position(|v| *v == value)
        .map_or(0, |at| at as i8 - middle as i8)
}

impl Recipe {
    /// What the settings ask for on a screen `width` by `height`; `None`
    /// where no picture is chosen.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(asked: &Asked<'_>, width: u16, height: u16) -> Result<Option<Self>, Error> {
        if asked.path.is_empty() {
            return Ok(None);
        }
        let at = settings::ANCHORS
            .iter()
            .position(|a| *a == asked.anchor)
            .unwrap_or(4);
        Ok(Some(Self {
            path: copy(asked.path)?,
            bytes: asked.bytes,
            modified: asked.modified,
            fitting: Fitting::of(asked.fit),
            across: (at % 3) as u8,
            down: (at / 3) as u8,
            zoom: asked.zoom.clamp(100, 300) as u32,
            lighten: asked.lighten.clamp(0, 60) as u32,
            brightness: step(asked.brightness, &settings::BRIGHTNESSES, 2),
            contrast: step(asked.contrast, &settings::CONTRASTS, 1),
            dotted: Render::of(asked.render) == Render::Dithered,
            grays: Render::of(asked.render) == Render::Grays,
            width,
            height,
        }))
    }

    /// The note kept beside the baked picture: the path by its hash (a long
    /// one must not make a long note) and everything else the picture is
    /// made of.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn key(&self) -> Result<String, Error> {
        let mut out = Out(String::new());
        write!(
            out,
            "{:016x}|{}|{}|{:?}|{}{}|{}|{}|{}{}|{}{}|{}x{}",
            hash(&self.path),
            self.bytes,
            self.modified,
            self.fitting,
            self.across,
            self.down,
            self.zoom,
            self.lighten,
            self.brightness,
            self.contrast,
            self.dotted,
            if self.grays { "g" } else { "" },
            self.width,
            self.height
        )
        .map_err(|_| Error::NoMemory)?;
        Ok(out.0)
    }
}

/// A 64-bit FNV-1a hash of `text`.
fn hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The tone curve of the picture: a gray in, the gray out. A step of
/// brightness lifts (or sinks) the middle grays most and keeps black and
/// white where they are; contrast stretches the grays about the middle.
/// Never falls as the gray rises; 0 and 0 give every gray back as it came.
#[must_use]
pub fn tone(brightness: i8, contrast: i8) -> [u8; 256] {
    let mut curve = [0u8; 256];
    for (gray, out) in curve.iter_mut().enumerate() {
        let gray = gray as i32;
        let lifted = gray + gray * (255 - gray) * i32::from(brightness) * 9 / (255 * 20);
        let times = 20 + i32::from(contrast) * if contrast < 0 { 5 } else { 7 };
        let stretched = 128 + (lifted - 128) * times / 20;
        *out = stretched.clamp(0, 255) as u8;
    }
    curve
}

/// The 4 by 4 pattern of the thresholds of an ordered dither.
const BAYER: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// `value` as black or white by the ordered dither's pattern at (`x`, `y`):
/// a gray comes as the share of white dots it asks for.
#[must_use]
pub fn dotted(value: u8, x: usize, y: usize) -> u8 {
    let threshold = u16::from(BAYER[y % 4][x % 4]) * 16 + 8;
    if u16::from(value) > threshold { 255 } else { 0 }
}

/// `value` paler by `percent`.
#[must_use]
pub fn lighten(value: u8, percent: u32) -> u8 {
    let v = u32::from(value);
    (v + (255 - v) * percent.min(100) / 100) as u8
}

/// Where the picture goes: its size on the screen, and the offset of its
/// top left corner (negative where it is cut).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plan {
    pub width: u32,
    pub height: u32,
    pub left: i32,
    pub top: i32,
}

/// Where a picture of `source` pixels (width, height) goes as `recipe` says.
#[must_use]
pub fn plan(recipe: &Recipe, source: (u32, u32)) -> Plan {
    let (sw, sh) = (u64::from(source.0.max(1)), u64::from(source.1.max(1)));
    let (w, h) = (u64::from(recipe.width), u64::from(recipe.height));
    let zoom = u64::from(recipe.zoom);
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
    Plan {
        width: pw,
        height: ph,
        left: place(i64::from(recipe.width) - i64::from(pw), recipe.across) as i32,
        top: place(i64::from(recipe.height) - i64::from(ph), recipe.down) as i32,
    }
}

/// How a picture as it is stored is turned to be upright: its rows and
/// columns swapped, then mirrored along either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Turn {
    pub transpose: bool,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Turn {
    /// The turn of EXIF orientation `orientation` (1 to 8).
    #[must_use]
    pub const fn of(orientation: u16) -> Self {
        let (transpose, flip_x, flip_y) = match orientation {
            2 => (false, true, false),
            3 => (false, true, true),
            4 => (false, false, true),
            5 => (true, false, false),
            6 => (true, true, false),
            7 => (true, true, true),
            8 => (true, false, true),
            _ => (false, false, false),
        };
        Self {
            transpose,
            flip_x,
            flip_y,
        }
    }

    /// The size a picture of `size` has once turned.
    #[must_use]
    pub const fn size(self, (width, height): (u32, u32)) -> (u32, u32) {
        if self.transpose {
            (height, width)
        } else {
            (width, height)
        }
    }
}

/// A part of a picture, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Crop {
    /// All of a picture of `size`.
    #[must_use]
    pub const fn all((width, height): (u32, u32)) -> Self {
        Self {
            x: 0,
            y: 0,
            width,
            height,
        }
    }
}

/// The part of a picture of `upright` pixels (the decoded one, upright) that
/// the screen shows as `plan` places it, in the coordinates of the picture
/// before it is mirrored, with a pixel to spare on each side.
#[must_use]
pub fn visible(recipe: &Recipe, plan: &Plan, upright: (u32, u32), turn: Turn) -> Crop {
    if recipe.fitting == Fitting::Tile {
        return Crop::all(upright);
    }
    let span = |left: i32, across: u32, screen: u16, size: u32, mirrored: bool| {
        let (left, across, size) = (i64::from(left), i64::from(across).max(1), i64::from(size));
        let from = (-left).max(0);
        let to = (i64::from(screen) - left).min(across);
        if to <= from {
            return (0, 1);
        }
        let start = (from * size / across - 1).max(0);
        let end = ((to * size + across - 1) / across + 1).min(size);
        let (start, end) = if mirrored {
            (size - end, size - start)
        } else {
            (start, end)
        };
        (start as u32, (end - start).max(1) as u32)
    };
    let (x, width) = span(plan.left, plan.width, recipe.width, upright.0, turn.flip_x);
    let (y, height) = span(plan.top, plan.height, recipe.height, upright.1, turn.flip_y);
    Crop {
        x,
        y,
        width,
        height,
    }
}

/// Reads the bytes of a file at an offset; the number of bytes read, or
/// `None` where the file cannot be read.
pub trait ReadAt {
    /// Fills `out` from `at`.
    fn read_at(&self, at: u64, out: &mut [u8]) -> Option<usize>;
}

/// The segments looked at at most: a frame comes after a few.
const SEGMENTS: usize = 256;
/// The entries of the first directory of the EXIF block looked at.
const ENTRIES: u64 = 64;

/// Calls `visit` with the marker, where the data begins and how long it is
/// for each segment of the JPEG, until it returns something or the scan
/// begins.
fn segments<T>(file: &impl ReadAt, mut visit: impl FnMut(u8, u64, u64) -> Option<T>) -> Option<T> {
    let mut soi = [0u8; 2];
    if file.read_at(0, &mut soi)? < 2 || soi != [0xff, 0xd8] {
        return None;
    }
    let mut at = 2u64;
    for _ in 0..SEGMENTS {
        let mut head = [0u8; 4];
        if file.read_at(at, &mut head)? < 4 || head[0] != 0xff {
            return None;
        }
        let marker = head[1];
        if marker == 0xff {
            at += 1;
            continue;
        }
        let length = u64::from(u16::from_be_bytes([head[2], head[3]]));
        if marker == 0xda || marker == 0xd9 || length < 2 {
            return None;
        }
        if let Some(found) = visit(marker, at + 4, length - 2) {
            return Some(found);
        }
        at += 2 + length;
    }
    None
}

/// The width and height of a JPEG's frame.
#[must_use]
pub fn jpeg_size(file: &impl ReadAt) -> Option<(u32, u32)> {
    segments(file, |marker, data, _| {
        if !matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            return None;
        }
        let mut frame = [0u8; 5];
        if file.read_at(data, &mut frame)? < 5 {
            return Some(None);
        }
        let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
        let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
        Some((width > 0 && height > 0).then_some((width, height)))
    })
    .flatten()
}

/// The orientation (1 to 8) a JPEG's EXIF block notes; 1, as it is, where it
/// notes none.
#[must_use]
pub fn jpeg_orientation(file: &impl ReadAt) -> u16 {
    segments(file, |marker, data, length| {
        if marker != 0xe1 || length < 14 {
            return None;
        }
        let mut head = [0u8; 14];
        if file.read_at(data, &mut head)? < 14 || &head[..6] != b"Exif\0\0" {
            return None;
        }
        let tiff = data + 6;
        let little = match &head[6..8] {
            b"II" => true,
            b"MM" => false,
            _ => return Some(1),
        };
        let word = |b: [u8; 2]| {
            if little {
                u16::from_le_bytes(b)
            } else {
                u16::from_be_bytes(b)
            }
        };
        let long = |b: [u8; 4]| {
            if little {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };
        let directory = u64::from(long([head[10], head[11], head[12], head[13]]));
        let mut count = [0u8; 2];
        if directory + 2 > length - 6 || file.read_at(tiff + directory, &mut count)? < 2 {
            return Some(1);
        }
        for i in 0..u64::from(word(count)).min(ENTRIES) {
            let mut entry = [0u8; 12];
            let at = tiff + directory + 2 + i * 12;
            if file.read_at(at, &mut entry)? < 12 {
                break;
            }
            if word([entry[0], entry[1]]) == 0x0112 {
                return Some(word([entry[8], entry[9]]));
            }
        }
        Some(1)
    })
    .filter(|o| (1..=8).contains(o))
    .unwrap_or(1)
}

/// Whether a file of the pictures folder is offered: not a folder, not
/// hidden, and a JPEG, PNG or GIF by its name.
#[must_use]
pub fn is_picture(name: &str, is_dir: bool) -> bool {
    !is_dir
        && !name.starts_with('.')
        && [".jpg", ".jpeg", ".png", ".gif"].iter().any(|extension| {
            name.len() >= extension.len()
                && name.as_bytes()[name.len() - extension.len()..]
                    .eq_ignore_ascii_case(extension.as_bytes())
        })
}

/// The order of names: without regard to case, character by character
/// (`char::to_lowercase`, which allocates nothing), then by their bytes so
/// that two names that differ only in case have a fixed order.
fn by_name(a: &str, b: &str) -> core::cmp::Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
        .then_with(|| a.cmp(b))
}

/// The names of the pictures of a folder, from its `entries` (name, folder
/// or not) in whatever order the card lists them: the alphabetically first
/// [`MOST`] of them as far as their names fit in [`NAMES`] bytes, sorted
/// without regard to case. The set is kept sorted while the folder is read
/// and the last name is dropped whenever it is too many or too long, so what
/// is kept does not depend on the order of the card (the interface-6 screen
/// kept the first 300 the card listed, then sorted them).
///
/// Cost: each insert moves at most about 7 KB (300 names of 24 bytes each in the
/// vector) and a binary search takes about 9 compares, so a folder listed in
/// descending order costs about N × (9 compares + one move + one allocation and
/// one free), which is fine in practice.
///
/// Names compare character by character lowercased (`char::to_lowercase`);
/// the interface-6 screen lowercases each whole name (`str::to_lowercase`),
/// which differs only where a Greek capital sigma ends a word (`ς` there, `σ`
/// here): only names that differ by that sigma can sort differently.
///
/// # Errors
/// [`Error::NoMemory`]; nothing is returned then.
pub fn pictures<'a>(entries: impl Iterator<Item = (&'a str, bool)>) -> Result<Vec<String>, Error> {
    let mut kept = Pictures::default();
    for (name, is_dir) in entries {
        kept.add(name, is_dir)?;
    }
    Ok(kept.finish())
}

/// The pictures of a folder as its entries come, page by page: what
/// [`pictures`] does for a whole listing at once.
#[derive(Debug, Default)]
pub struct Pictures {
    found: Vec<String>,
    bytes: usize,
    /// The smallest name dropped so far: no name from there on is kept, even
    /// one that would fit, or what is kept would depend on the card's order.
    cut: Option<String>,
}

impl Pictures {
    /// Considers one entry of the folder.
    ///
    /// # Errors
    /// [`Error::NoMemory`]; what was kept stays.
    pub fn add(&mut self, name: &str, is_dir: bool) -> Result<(), Error> {
        if !is_picture(name, is_dir)
            || self
                .cut
                .as_deref()
                .is_some_and(|cut| by_name(name, cut) != core::cmp::Ordering::Less)
        {
            return Ok(());
        }
        let at = match self.found.binary_search_by(|kept| by_name(kept, name)) {
            // The same name twice: once.
            Ok(_) => return Ok(()),
            Err(at) => at,
        };
        self.found.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.found.insert(at, copy(name)?);
        self.bytes += name.len();
        while self.found.len() > MOST || self.bytes > NAMES {
            let Some(last) = self.found.pop() else { break };
            self.bytes -= last.len();
            self.cut = Some(last);
        }
        Ok(())
    }

    /// The names kept, sorted.
    #[must_use]
    pub fn finish(self) -> Vec<String> {
        self.found
    }
}

/// When a bar shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Visible {
    #[default]
    Off,
    /// After a press, for a few seconds.
    Auto,
    On,
}

impl Visible {
    /// What a choice names (on for any other value).
    #[must_use]
    pub fn of(value: &str) -> Self {
        match settings::BARS.iter().position(|b| *b == value) {
            Some(0) => Self::Off,
            Some(1) => Self::Auto,
            _ => Self::On,
        }
    }
}

/// What the bar is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// A plate as light as the page.
    Normal,
    /// A black plate.
    Inverted,
    /// A light gray plate.
    Light,
    /// A dark gray plate.
    Dark,
    /// No plate: the items on the picture, in the ink that reads on it.
    Clear,
}

impl Look {
    /// What a choice names (normal for any other value).
    #[must_use]
    pub fn of(value: &str) -> Self {
        match settings::BAR_LOOKS.iter().position(|l| *l == value) {
            Some(1) => Self::Inverted,
            Some(2) => Self::Light,
            Some(3) => Self::Dark,
            Some(4) => Self::Clear,
            _ => Self::Normal,
        }
    }

    /// The plate's gray and the ink's on it (0 black to 255 white); no plate
    /// for [`Look::Clear`].
    #[must_use]
    pub const fn plate(self) -> Option<(u8, u8)> {
        match self {
            Self::Normal => Some((255, 0)),
            Self::Inverted => Some((0, 255)),
            Self::Light => Some((165, 0)),
            Self::Dark => Some((85, 255)),
            Self::Clear => None,
        }
    }
}

/// The ink (0 black or 255 white) that reads over a picture `brightness`
/// bright.
#[must_use]
pub fn ink_over(brightness: Option<u8>) -> u8 {
    if brightness.is_some_and(|b| b < 128) {
        255
    } else {
        0
    }
}

/// Where the bar is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placing {
    pub bottom: bool,
    pub compact: bool,
    /// 0 left, 1 middle, 2 right.
    pub align: u8,
}

impl Placing {
    /// From the choices of the bar's place, shape and side.
    #[must_use]
    pub fn of(at: &str, shape: &str, align: &str) -> Self {
        Self {
            bottom: at == settings::BAR_ATS[1],
            compact: shape == settings::BAR_SHAPES[1],
            align: settings::BAR_ALIGNS
                .iter()
                .position(|a| *a == align)
                .unwrap_or(2) as u8,
        }
    }
}

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Area {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

/// Where the bar's plate is, its strip being `strip` (the whole width of the
/// screen, at the top or the bottom) and `margin` the room to keep at the
/// sides. Sums are made in `u32` and kept to what a `u16` holds, so that no
/// strip can wrap.
#[must_use]
pub fn plate_area(placing: Placing, strip: Area, margin: u16) -> Area {
    let clamp = |value: u32| u16::try_from(value).unwrap_or(u16::MAX);
    let full_x = u32::from(strip.x) + u32::from(margin);
    let full_width = u32::from(strip.width).saturating_sub(2 * u32::from(margin));
    let (x, width) = if placing.compact {
        let width = full_width * 55 / 100;
        let x = match placing.align {
            0 => full_x,
            1 => full_x + (full_width - width) / 2,
            _ => full_x + full_width - width,
        };
        (x, width)
    } else {
        (full_x, full_width)
    };
    Area {
        x: clamp(x),
        y: strip.y,
        width: clamp(width),
        height: strip.height,
    }
}

#[cfg(test)]
mod tests;
