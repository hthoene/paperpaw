//! QR codes and pictures (covers, illustrations) drawn into an area.

#[allow(
    clippy::wildcard_imports,
    reason = "a part of the module, which shares its imports"
)]
use super::*;

// ---- QR codes ----------------------------------------------------------

/// Draws `qr` as large as fits in `area`, centred, each module a whole
/// number of pixels, with the four modules of white around it scanners
/// expect; returns where it went with that white, or `None` (nothing
/// drawn) if it does not fit.
pub fn draw_qr(canvas: &mut Canvas<'_>, qr: &crate::qr::QrCode, area: Rect) -> Option<Rect> {
    let modules = u16::try_from(qr.size()).ok()?.checked_add(8)?;
    let scale = area.width.min(area.height) / modules;
    if scale == 0 {
        return None;
    }
    let side = scale * modules;
    let x = area.x + (area.width - side) / 2;
    let y = area.y + (area.height - side) / 2;
    let (x0, y0) = (x + 4 * scale, y + 4 * scale);
    canvas.fill(Rect::new(x, y, side, side), Color::WHITE);
    for my in 0..qr.size() {
        for mx in 0..qr.size() {
            if qr.dark(mx, my) {
                canvas.fill_rect(
                    i32::from(x0 + mx as u16 * scale),
                    i32::from(y0 + my as u16 * scale),
                    u32::from(scale),
                    u32::from(scale),
                    Color::BLACK,
                );
            }
        }
    }
    Some(Rect::new(x, y, side, side))
}

// ---- Images ------------------------------------------------------------

/// Draws a decoded image (e.g. a book's cover or an illustration), gray
/// or in colour, scaled into `area`: each screen row reads one source row
/// from storage, and columns are averaged when the image is reduced.
/// Colour shows as colour on colour screens, as gray elsewhere. Where the
/// frame has few grays, an opaque picture is diffused into them
/// ([`Atkinson`](paperpaw_graphics::diffuse::Atkinson)) from its first
/// row on, so that each band and each part drawn again shows the same
/// pixels. A band goes on where the band above left it; a part drawn
/// again reads its rows above the clip for that, but does not draw them.
/// Elsewhere only the rows inside the clip are read.
pub fn draw_image(canvas: &mut Canvas<'_>, storage: &dyn Storage, image: &ImageRef, area: Rect) {
    draw_image_over(canvas, storage, image, area, 255);
}

/// Draws `image` into `area` as [`draw_image`] does, as a small picture
/// (a cover in a list or a widget): it does not count as a picture that
/// leaves ghosts on e-paper, which a full refresh clears after large
/// ones.
pub fn draw_small_image(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    image: &ImageRef,
    area: Rect,
) {
    canvas.small_pictures(|canvas| draw_image(canvas, storage, image, area));
}

/// Draws a decoded image over what is drawn in `area`, as
/// [`draw_image`]: a picture with transparency
/// ([`image::picture_with_alpha`](crate::image::picture_with_alpha))
/// shows what is below through its clear parts, blended at its edges and
/// dithered as pictures are; `opacity` (0 to 255) lets what is below show
/// through all of it, e.g. a picture over the page read. Any screen,
/// widget or overlay may draw so; rows are read and blended one at a
/// time, so little memory is taken.
pub fn draw_image_over(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    image: &ImageRef,
    area: Rect,
    opacity: u8,
) {
    // Frames are drawn in bands: nothing read for those it is not in, nor
    // where it is only described.
    if area.intersection(&canvas.clip()).is_none() || opacity == 0 {
        return;
    }
    if canvas.is_blind() {
        return canvas.note_picture(area);
    }
    // Its kind from one read of its header, band by band.
    let (kind, width, height) = match crate::image::Decoded::peek(storage, &image.path) {
        Some(crate::image::Decoded::Alpha(file)) => (PictureKind::Alpha, file.width, file.height),
        Some(crate::image::Decoded::Color(file)) => (PictureKind::Color, file.width, file.height),
        _ => (PictureKind::Gray, image.width, image.height),
    };
    // This drawer has always drawn what it could and said nothing.
    let _ = draw_picture(
        canvas,
        storage,
        &Picture {
            path: &image.path,
            kind,
            width,
            height,
            scratch_max: usize::MAX,
            tones: Tones::Panel,
        },
        area,
        opacity,
        true,
    );
}

/// What kind of decoded file a picture is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PictureKind {
    /// One byte a pixel (`PPGI`).
    Gray,
    /// Three bytes a pixel (`PPCI`).
    Color,
    /// Gray and opacity, two bytes a pixel (`PPGA`).
    Alpha,
}

/// How a picture is drawn on a panel with few grays (`canvas.draw_image` flags
/// bits 3 and 4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Tones {
    /// The panel's own rule: diffused into the grays it has.
    #[default]
    Panel,
    /// Black and white dots, also where the panel has grays.
    Dots,
    /// The grays the panel has (a panel with many grays, or colour, draws as
    /// it always does).
    Grays,
}

/// A decoded picture file the host has checked ([`probe_picture`]).
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct Picture<'a> {
    /// The decoded file in storage.
    pub path: &'a str,
    /// What its rows hold.
    pub kind: PictureKind,
    /// Width in pixels, as its header says.
    pub width: u32,
    /// Height in pixels, as its header says.
    pub height: u32,
    /// The most heap the draw may take for its one transient block, the
    /// device's (32 KiB on the X4, more where there is memory): a draw that
    /// needs more is `OutOfMemory` with nothing drawn.
    pub scratch_max: usize,
    /// How it is drawn on a panel with few grays.
    pub tones: Tones,
}

impl<'a> Picture<'a> {
    /// A picture of `kind` at `path`, `width` × `height` pixels (as
    /// [`probe_picture`] found them), whose draw may take `scratch_max`
    /// bytes of heap for its one transient block.
    #[must_use]
    pub const fn new(
        path: &'a str,
        kind: PictureKind,
        width: u32,
        height: u32,
        scratch_max: usize,
    ) -> Self {
        Self {
            path,
            kind,
            width,
            height,
            scratch_max,
            tones: Tones::Panel,
        }
    }

    /// The picture drawn in `tones` (black and white dots, or the panel's
    /// grays) instead of the panel's own rule.
    #[must_use]
    pub const fn with_tones(mut self, tones: Tones) -> Self {
        self.tones = tones;
        self
    }
}

/// The kind and size of the decoded picture file at `path`, only if its size
/// is what its header says: one header read, no allocation. The cache
/// image formats are `PPGI` (gray), `PPCI` (colour, 3 bytes a pixel) and
/// `PPGA` (gray and opacity, 2), after the 12-byte header.
///
/// # Errors
/// `Io` where the file cannot be read, is shorter than its header or than
/// its header says it is; `Invalid` where the header is not one of those,
/// has no size, or the file is longer than it says.
pub fn probe_picture(
    storage: &dyn Storage,
    path: &str,
) -> Result<(PictureKind, u32, u32), crate::image::ImageError> {
    use crate::image::ImageError;
    let mut header = [0_u8; 12];
    let read = storage
        .read_at(path, 0, &mut header)
        .map_err(|_| ImageError::Io)?;
    if read != header.len() {
        return Err(ImageError::Io);
    }
    let width = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);
    let height = u32::from_le_bytes([header[8], header[9], header[10], header[11]]);
    let (kind, bytes) = match &header[..4] {
        b"PPGI" => (PictureKind::Gray, 1_u64),
        b"PPCI" => (PictureKind::Color, 3),
        b"PPGA" => (PictureKind::Alpha, 2),
        _ => return Err(ImageError::Invalid("not a decoded picture")),
    };
    if width == 0 || height == 0 {
        return Err(ImageError::Invalid("a picture with no size"));
    }
    let expected = (u64::from(width) * u64::from(height))
        .checked_mul(bytes)
        .and_then(|n| n.checked_add(HEADER))
        .ok_or(ImageError::Invalid("a picture too large"))?;
    let size = storage.size(path).map_err(|_| ImageError::Io)?;
    match size.cmp(&expected) {
        core::cmp::Ordering::Equal => Ok((kind, width, height)),
        core::cmp::Ordering::Less => Err(ImageError::Io),
        core::cmp::Ordering::Greater => {
            Err(ImageError::Invalid("a picture longer than its header"))
        }
    }
}

/// A zeroed buffer, `ImageError::OutOfMemory` where there is no room.
pub(super) fn grab(len: usize) -> Result<Vec<u8>, crate::image::ImageError> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(len)
        .map_err(|_| crate::image::ImageError::OutOfMemory)?;
    bytes.resize(len, 0);
    Ok(bytes)
}

/// [`draw_image_over`] for a checked picture ([`probe_picture`]) with every
/// allocation fallible and every failure said: `OutOfMemory` (nothing is
/// drawn: the buffers are taken first), `Io` (a row that cannot be read:
/// the bands before it are drawn, none after). Nothing is drawn on a frame
/// that is only described or where the picture is not in the clip or has
/// no opacity.
pub fn try_draw_picture(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    picture: &Picture<'_>,
    area: Rect,
    opacity: u8,
) -> Result<(), crate::image::ImageError> {
    draw_picture(canvas, storage, picture, area, opacity, false)
}

/// The columns (from, to) a picture is made over: a dithered one over all of
/// its rectangle at `left`, `w` wide (up to `DIFFUSE_WIDTH_MAX`), also where
/// the clip shows only the columns `shown`: the error runs along the row from
/// its left end, so a part drawn alone must come out as it does in the whole.
/// Only the shown columns are drawn. Where the device's `budget` does not
/// hold the buffers of the full width (`wanted` says what a width takes),
/// only the shown columns are made: that part may then dither a little
/// differently from the same part of a whole draw.
fn made_columns(
    dithered: bool,
    (left, w): (i32, usize),
    shown: (i32, i32),
    (budget, wanted): (usize, impl Fn(usize) -> usize),
) -> (i32, i32) {
    if dithered && w <= DIFFUSE_WIDTH_MAX && wanted(w) <= budget {
        (left, left + w as i32)
    } else {
        shown
    }
}

/// The widest rectangle a dithered picture is made over in full when the clip
/// shows a part of it (the longest side of any picture is 1,600).
const DIFFUSE_WIDTH_MAX: usize = 1_600;

/// [`try_draw_picture`], where `retain` lets the frame keep the dithering
/// error from band to band ([`Canvas::keep_diffusion`], freed with the
/// frame); the host's draw keeps nothing between calls.
fn draw_picture(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    picture: &Picture<'_>,
    area: Rect,
    opacity: u8,
    retain: bool,
) -> Result<(), crate::image::ImageError> {
    if area.intersection(&canvas.clip()).is_none() || opacity == 0 {
        return Ok(());
    }
    if canvas.is_blind() {
        canvas.note_picture(area);
        return Ok(());
    }
    let source = ImageSource {
        kind: picture.kind,
        path: picture.path,
        width: picture.width,
        height: picture.height,
    };
    // On gray frames an opaque colour picture is drawn as colour rows the
    // frame turns to gray.
    if picture.kind == PictureKind::Color
        && opacity == 255
        && canvas.picture_levels().is_none()
        && picture.tones != Tones::Dots
    {
        return draw_color_image(canvas, storage, &source, area, picture.scratch_max);
    }
    draw_source(
        canvas,
        storage,
        &source,
        area,
        opacity,
        (retain, picture.scratch_max),
        picture.tones,
    )
}

/// Where the rows of a picture drawn as gray come from: a checked decoded
/// file ([`probe_picture`]), borrowed.
struct ImageSource<'a> {
    kind: PictureKind,
    path: &'a str,
    width: u32,
    height: u32,
}

/// Bytes of the header of a decoded picture file.
const HEADER: u64 = 12;

impl ImageSource<'_> {
    fn size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    /// Bytes a pixel in its rows.
    fn bytes(&self) -> usize {
        match self.kind {
            PictureKind::Gray => 1,
            PictureKind::Color => 3,
            PictureKind::Alpha => 2,
        }
    }

    /// Reads rows from `y` on into `out`, as many whole ones as fit and the
    /// picture has, in one read; the rows read, 0 if none could be.
    fn read_rows(&self, storage: &dyn Storage, y: u32, out: &mut [u8]) -> u32 {
        let row = self.width as usize * self.bytes();
        let rows = (out.len() / row.max(1)).min(self.height.saturating_sub(y) as usize);
        if rows == 0 {
            return 0;
        }
        let want = &mut out[..rows * row];
        let mut done = 0;
        while done < want.len() {
            let at = HEADER + u64::from(y) * row as u64 + done as u64;
            match storage.read_at(self.path, at, &mut want[done..]) {
                Ok(0) | Err(_) => return 0,
                Ok(n) => done += n,
            }
        }
        rows as u32
    }

    /// A row of it as gray and opacity, two bytes a pixel, into `out`.
    fn gray_alpha(&self, row: &[u8], out: &mut [u8]) {
        match self.kind {
            PictureKind::Alpha => out.copy_from_slice(row),
            PictureKind::Gray => {
                for (pixel, &g) in out.as_chunks_mut::<2>().0.iter_mut().zip(row) {
                    *pixel = [g, 255];
                }
            }
            PictureKind::Color => {
                let rgb = row.as_chunks::<3>().0;
                for (pixel, c) in out.as_chunks_mut::<2>().0.iter_mut().zip(rgb) {
                    let luma = crate::image::luma(c[0].into(), c[1].into(), c[2].into());
                    *pixel = [luma, 255];
                }
            }
        }
    }
}

/// Where output pixel `i` of `out` takes its source from when an image
/// of `source` pixels is made larger: the two pixels around it and how
/// far towards the second (in 256ths), as the pixels' centres lie.
fn between(i: usize, out: usize, source: usize) -> (usize, usize, u32) {
    let at = ((2 * i + 1) * source * 128 / out).saturating_sub(128);
    let first = (at >> 8).min(source - 1);
    (first, (first + 1).min(source - 1), (at & 255) as u32)
}

/// Where the diffusion of a call starts: where the band above left it, from
/// the first row on, or none where memory for it is short (a frame dithers in
/// place; a call that keeps nothing says so).
fn start_diffusion(
    canvas: &mut Canvas<'_>,
    levels: Option<u8>,
    (key, shown, top): (u64, i32, i32),
    (vw, retain): (usize, bool),
) -> Result<(i32, Option<(paperpaw_graphics::diffuse::Atkinson, u8)>), crate::image::ImageError> {
    let Some(levels) = levels else {
        return Ok((shown, None));
    };
    match retain.then(|| canvas.take_diffusion(key, shown)).flatten() {
        Some(atkinson) if atkinson.width() == vw => Ok((shown, Some((atkinson, levels)))),
        _ => match paperpaw_graphics::diffuse::Atkinson::new(vw) {
            Some(atkinson) => Ok((top, Some((atkinson, levels)))),
            None if retain => Ok((shown, None)),
            None => Err(crate::image::ImageError::OutOfMemory),
        },
    }
}

/// [`draw_image_over`] for a picture drawn as gray, each axis on its
/// own: made smaller, each pixel the mean of the columns it covers (one
/// row read for it), its gray weighted by how opaque each is; made
/// larger, interpolated between the pixels around it (bilinear, in whole
/// numbers). Opaque and not to be seen through, it is drawn as gray
/// rows.
fn draw_source(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    source: &ImageSource<'_>,
    area: Rect,
    opacity: u8,
    (retain, budget): (bool, usize),
    tones: Tones,
) -> Result<(), crate::image::ImageError> {
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (usize::from(area.width), usize::from(area.height));
    let (sw, sh) = source.size();
    if w == 0 || h == 0 || sw == 0 || sh == 0 {
        return Ok(());
    }
    let clip = canvas.clip();
    let shown = top.max(i32::from(clip.y));
    let last = (top + h as i32).min(clip.bottom() as i32);
    // Only the columns the clip shows are made: the rest of a rectangle
    // many times the screen costs nothing.
    let (vx0, vx1) = (
        left.max(i32::from(clip.x)),
        (left + w as i32).min(i32::from(clip.x) + i32::from(clip.width)),
    );
    if shown >= last || vx0 >= vx1 {
        return Ok(());
    }
    // An axis made larger interpolates, one made smaller averages.
    let (wider, taller) = (w > sw, h > sh);
    let blended = opacity < 255 || source.kind == PictureKind::Alpha;
    // Diffused into the frame's few grays from the first row on, where
    // what it blends with is not its own (see `draw_image`), or from
    // where the band above left it; a frame dithers in place where memory
    // for it is short.
    let levels = match tones {
        // Black and white dots where nothing under the picture shows through.
        Tones::Dots => (!blended).then_some(2),
        _ => canvas.picture_levels().filter(|_| !blended),
    };
    let buffers = |vw| scratch_bytes(source, (sw, vw), (wider, taller), blended, levels.is_some());
    let (mx0, mx1) = made_columns(levels.is_some(), (left, w), (vx0, vx1), (budget, buffers));
    let (skip, vw) = ((mx0 - left) as usize, (mx1 - mx0) as usize);
    let key = levels.map_or(0, |levels| {
        picture_key(source, canvas.origin(), area, (mx0, vw), levels)
    });
    // Every buffer of this call, summed before any is taken.
    let wanted = buffers(vw);
    // The device's limit (`draw_image_over` has no limit of its own).
    if wanted > budget {
        return Err(crate::image::ImageError::OutOfMemory);
    }
    let (first, mut diffusion) = start_diffusion(canvas, levels, (key, shown, top), (vw, retain))?;
    let mut diffused = grab(if diffusion.is_some() { vw } else { 0 })?;
    let mut rows = RowChunk::new(sw * source.bytes())?;
    // The source row a screen row comes from, as gray and opacity; made
    // taller, the one below it too, and the two mixed.
    let mut upper = grab(sw * 2)?;
    let mut lower = grab(if taller { sw * 2 } else { 0 })?;
    let mut mixed = grab(if taller { sw * 2 } else { 0 })?;
    let (mut held_upper, mut held_lower) = (None, None);
    let mut row = grab(vw * if blended { 2 } else { 1 })?;
    let columns = Columns::new(vw, skip, w, sw, wider)?;
    let mut made = None;
    for y in first..last {
        let oy = (y - top) as usize;
        let (above, below, ty) = if taller {
            between(oy, h, sh)
        } else {
            let sy = oy * sh / h;
            (sy, sy, 0)
        };
        if made != Some((above, below, ty)) {
            load(
                &mut rows,
                source,
                storage,
                above,
                &mut upper,
                &mut held_upper,
            )?;
            let between_rows = ty > 0 && below != above;
            if between_rows {
                load(
                    &mut rows,
                    source,
                    storage,
                    below,
                    &mut lower,
                    &mut held_lower,
                )?;
                mix(&upper, &lower, ty, &mut mixed);
            }
            let line: &[u8] = if between_rows { &mixed } else { &upper };
            made_row(line, &columns, blended, &mut row);
            made = Some((above, below, ty));
        }
        match &mut diffusion {
            Some((atkinson, levels)) => {
                atkinson.row(&row, *levels, |x, gray| diffused[x] = gray);
                if y >= shown {
                    let at = (vx0 - mx0) as usize;
                    canvas.draw_gray_row(vx0, y, &diffused[at..at + (vx1 - vx0) as usize]);
                }
            }
            None => draw_row(canvas, vx0, y, &row, blended, opacity),
        }
    }
    if let Some((atkinson, _)) = diffusion
        && retain
        && last < top + h as i32
    {
        canvas.keep_diffusion(key, last, atkinson);
    }
    Ok(())
}

/// The heap [`draw_source`] takes for a picture `sw` wide in `vw` visible
/// columns: the row chunk, the source rows made, the screen row, the column
/// table and, where it dithers, its error rows and gray row.
fn scratch_bytes(
    source: &ImageSource<'_>,
    (sw, vw): (usize, usize),
    (wider, taller): (bool, bool),
    blended: bool,
    dithered: bool,
) -> usize {
    // Saturating: the sizes come from a picture's own header.
    let part = |n: usize, each: usize| n.saturating_mul(each);
    ROWS_BYTES
        .max(part(sw, source.bytes()))
        .saturating_add(part(sw, 2 * if taller { 3 } else { 1 }))
        .saturating_add(part(vw, if blended { 2 } else { 1 }))
        .saturating_add(if wider {
            part(vw, 6)
        } else {
            part(vw.saturating_add(1), 2)
        })
        .saturating_add(if dithered { part(vw, 5) } else { 0 })
}

/// Which picture is diffused where (see [`Canvas::take_diffusion`]): the
/// same file in the same place on the screen, as large and in as many
/// grays.
fn picture_key(
    source: &ImageSource<'_>,
    origin: Point,
    area: Rect,
    visible: (i32, usize),
    levels: u8,
) -> u64 {
    let path = source.path;
    let mut bytes = [0u8; 25];
    bytes[..2].copy_from_slice(&area.x.to_le_bytes());
    bytes[2..4].copy_from_slice(&area.y.to_le_bytes());
    bytes[4..6].copy_from_slice(&area.width.to_le_bytes());
    bytes[6..8].copy_from_slice(&area.height.to_le_bytes());
    bytes[8..12].copy_from_slice(&origin.x.to_le_bytes());
    bytes[12..16].copy_from_slice(&origin.y.to_le_bytes());
    bytes[16] = levels;
    bytes[17..21].copy_from_slice(&visible.0.to_le_bytes());
    bytes[21..25].copy_from_slice(&(visible.1 as u32).to_le_bytes());
    paperpaw_digest::fnv1a64(path.as_bytes()) ^ paperpaw_digest::fnv1a64(bytes).rotate_left(1)
}

/// Reads source row `y` into `into` as gray and opacity, unless it holds
/// it already (`held`); `Io` where it cannot be read.
fn load(
    rows: &mut RowChunk,
    source: &ImageSource<'_>,
    storage: &dyn Storage,
    y: usize,
    into: &mut [u8],
    held: &mut Option<usize>,
) -> Result<(), crate::image::ImageError> {
    if *held == Some(y) {
        return Ok(());
    }
    let Some(read) = rows.get(y as u32, |y, out| source.read_rows(storage, y, out)) else {
        return Err(crate::image::ImageError::Io);
    };
    source.gray_alpha(read, into);
    *held = Some(y);
    Ok(())
}

/// `upper` and `lower` (gray and opacity) mixed into `out`, `t` (in
/// 256ths) towards `lower`, the gray weighted by opacity.
fn mix(upper: &[u8], lower: &[u8], t: u32, out: &mut [u8]) {
    let pairs = upper
        .as_chunks::<2>()
        .0
        .iter()
        .zip(lower.as_chunks::<2>().0);
    for (pixel, (&[g0, a0], &[g1, a1])) in out.as_chunks_mut::<2>().0.iter_mut().zip(pairs) {
        let (a0, a1) = (u32::from(a0) * (256 - t), u32::from(a1) * t);
        let opaque = a0 + a1;
        let weighted = u32::from(g0) * a0 + u32::from(g1) * a1;
        let gray = weighted
            .checked_div(opaque)
            .map_or(255, |g| g.min(255) as u8);
        *pixel = [gray, (opaque / 256).min(255) as u8];
    }
}

/// Where each screen column takes its pixels from in a source row, worked
/// out once a picture (not divisions a pixel a row, slow on chips
/// without them): made wider, the two around it and how far towards the
/// second (256ths); else the first of those it covers, the last entry the
/// row's end.
enum Columns {
    Between(Vec<(u16, u16, u16)>),
    Covers(Vec<u16>),
}

impl Columns {
    /// For the `vw` columns from column `skip` of a picture `w` wide.
    fn new(
        vw: usize,
        skip: usize,
        w: usize,
        sw: usize,
        wider: bool,
    ) -> Result<Self, crate::image::ImageError> {
        let no_room = || crate::image::ImageError::OutOfMemory;
        if wider {
            let mut at = Vec::new();
            at.try_reserve_exact(vw).map_err(|_| no_room())?;
            at.extend((skip..skip + vw).map(|x| {
                let (x0, x1, t) = between(x, w, sw);
                (x0 as u16, x1 as u16, t as u16)
            }));
            Ok(Columns::Between(at))
        } else {
            let mut starts = Vec::new();
            starts.try_reserve_exact(vw + 1).map_err(|_| no_room())?;
            starts.extend((skip..=skip + vw).map(|x| (x * sw / w).min(sw) as u16));
            Ok(Columns::Covers(starts))
        }
    }
}

/// A screen row `out` made of the source row `line` (gray and opacity):
/// interpolated between columns where it is made wider, each pixel the
/// mean of the columns it covers where it is not (see [`Columns`]).
fn made_row(line: &[u8], columns: &Columns, blended: bool, out: &mut [u8]) {
    let w = out.len() / if blended { 2 } else { 1 };
    let sw = line.len() / 2;
    let pixel = |i: usize| (u32::from(line[i * 2]), u32::from(line[i * 2 + 1]));
    for x in 0..w {
        let (weighted, opaque, count) = match columns {
            Columns::Between(at) => {
                let (x0, x1, tx) = at[x];
                let ((g0, a0), (g1, a1)) = (pixel(usize::from(x0)), pixel(usize::from(x1)));
                let tx = u32::from(tx);
                let (a0, a1) = (a0 * (256 - tx), a1 * tx);
                (g0 * a0 + g1 * a1, a0 + a1, 256)
            }
            Columns::Covers(starts) => {
                let from = usize::from(starts[x]);
                let to = usize::from(starts[x + 1]).max(from + 1).min(sw);
                let (mut weighted, mut opaque) = (0u32, 0u32);
                for i in from..to {
                    let (g, a) = pixel(i);
                    weighted += g * a;
                    opaque += a;
                }
                (weighted, opaque, (to - from) as u32)
            }
        };
        let gray = weighted
            .checked_div(opaque)
            .map_or(255, |g| g.min(255) as u8);
        if blended {
            out[x * 2] = gray;
            out[x * 2 + 1] = (opaque / count).min(255) as u8;
        } else {
            out[x] = gray;
        }
    }
}

/// A row of [`draw_source`]: gray, or gray and opacity blended over what
/// is drawn.
fn draw_row(canvas: &mut Canvas<'_>, left: i32, y: i32, row: &[u8], blended: bool, opacity: u8) {
    if blended {
        canvas.draw_gray_alpha_row(left, y, row, opacity);
    } else {
        canvas.draw_gray_row(left, y, row);
    }
}

/// Bytes of a picture's rows read at a time.
pub(super) const ROWS_BYTES: usize = 8192;

/// Rows of a picture read several at a time: a card serves whole blocks
/// in one command faster than a row at a time.
pub(super) struct RowChunk {
    data: Vec<u8>,
    row: usize,
    first: u32,
    count: u32,
}

impl RowChunk {
    /// For rows of `row` bytes.
    fn new(row: usize) -> Result<Self, crate::image::ImageError> {
        let rows = (ROWS_BYTES / row.max(1)).max(1);
        Ok(Self {
            data: grab(rows * row)?,
            row,
            first: 0,
            count: 0,
        })
    }

    /// Row `y`, read with those after it unless at hand; `read` reads rows
    /// from a row on and says how many.
    fn get(&mut self, y: u32, read: impl FnOnce(u32, &mut [u8]) -> u32) -> Option<&[u8]> {
        if !(self.first..self.first + self.count).contains(&y) {
            self.first = y;
            self.count = read(y, &mut self.data);
            if self.count == 0 {
                return None;
            }
        }
        let at = (y - self.first) as usize * self.row;
        Some(&self.data[at..at + self.row])
    }
}

/// [`draw_image`] for a colour image (a checked [`ImageSource`] of kind
/// colour), fallible as [`try_draw_picture`] is.
fn draw_color_image(
    canvas: &mut Canvas<'_>,
    storage: &dyn Storage,
    file: &ImageSource<'_>,
    area: Rect,
    budget: usize,
) -> Result<(), crate::image::ImageError> {
    let (left, top) = (i32::from(area.x), i32::from(area.y));
    let (w, h) = (i32::from(area.width), i32::from(area.height));
    if w <= 0 || h <= 0 {
        return Ok(());
    }
    let clip = canvas.clip();
    let first = top.max(i32::from(clip.y));
    let last = (top + h).min(clip.bottom() as i32);
    if first >= last {
        return Ok(());
    }
    // Only the columns the clip shows are made.
    let (vx0, vx1) = (
        left.max(i32::from(clip.x)),
        (left + w).min(i32::from(clip.x) + i32::from(clip.width)),
    );
    if vx0 >= vx1 {
        return Ok(());
    }
    let (skip, vw) = ((vx0 - left) as usize, (vx1 - vx0) as usize);
    let sw = file.width as usize;
    if ROWS_BYTES.max(sw.saturating_mul(3)).saturating_add(vw * 3) > budget {
        return Err(crate::image::ImageError::OutOfMemory);
    }
    let mut rows = RowChunk::new(sw * 3)?;
    let mut row = grab(vw * 3)?;
    let mut loaded = None;
    for y in first..last {
        let sy = ((y - top) as u64 * u64::from(file.height) / h as u64) as u32;
        if loaded != Some(sy) {
            let Some(source) = rows.get(sy, |y, out| file.read_rows(storage, y, out)) else {
                return Err(crate::image::ImageError::Io);
            };
            loaded = Some(sy);
            for x in 0..vw {
                let at = x + skip;
                let from = at * sw / w as usize;
                let to = ((at + 1) * sw / w as usize).max(from + 1).min(sw);
                for c in 0..3 {
                    let sum: u32 = (from..to).map(|i| u32::from(source[i * 3 + c])).sum();
                    row[x * 3 + c] = (sum / (to - from) as u32) as u8;
                }
            }
        }
        canvas.draw_rgb_row(vx0, y, &row);
    }
    Ok(())
}

/// The largest size of `image` that fits `width` × `height`, keeping its
/// proportions.
pub fn fit_image(image: &ImageRef, width: u16, height: u16) -> (u16, u16) {
    if image.width == 0 || image.height == 0 {
        return (0, 0);
    }
    let by_width = u64::from(width) * u64::from(image.height) / u64::from(image.width);
    if by_width <= u64::from(height) {
        (width, by_width as u16)
    } else {
        let w = u64::from(height) * u64::from(image.width) / u64::from(image.height);
        (w as u16, height)
    }
}
