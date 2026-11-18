//! Panels of a comic page, found in its decoded gray picture by the white
//! gutters between them (a recursive cut: rows, then columns, then rows
//! again), and a panel drawn on a canvas of the page's size, as large as the
//! canvas allows, for a reader to show it as a page of its own.
//!
//! Only rows are read, a few at a time: the heap holds a few buffers of the
//! page's width (about 6 KiB for a page 480 wide), never the picture.

use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::document::DocumentError;
use paperpaw_sdk::image::{GrayFile, GrayWriter, ImageSink};

use crate::fallible;

/// Most panels a page has; a page of more is shown whole. Also the number
/// of positions a page has in panel mode.
pub(crate) const MAX_PANELS: usize = 16;
/// Pictures larger than this are not searched (a decoded page is never).
const MAX_SIDE: u32 = 4096;
/// Pictures smaller than this have no panels worth stepping through.
const MIN_SIDE: u32 = 64;
/// A pixel at least this bright is paper.
const PAPER: u8 = 224;
/// Bytes of rows read at once.
const READ: usize = 4096;
/// Rectangles waiting to be cut, and cuts made, at most.
const WAITING: usize = 48;
const CUTS: usize = 96;
/// A rectangle is cut this many times at most one inside the other.
const DEPTH: u8 = 6;

/// A panel: its place on the page, in pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Panel {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

/// Bytes of a panel in the list ([`encode`]).
const RECORD: usize = 8;

/// The panels as a file keeps them: their number, then four `u16`s each.
pub(crate) fn encode(panels: &[Panel]) -> Result<Vec<u8>, DocumentError> {
    let mut out = Vec::new();
    fallible::reserve_exact(&mut out, 1 + RECORD * panels.len())?;
    out.push(panels.len().min(MAX_PANELS) as u8);
    for p in panels.iter().take(MAX_PANELS) {
        for v in [p.x, p.y, p.w, p.h] {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    Ok(out)
}

/// The panels a file keeps; none for one that is not whole.
pub(crate) fn decode(bytes: &[u8]) -> Result<Vec<Panel>, DocumentError> {
    let mut out = Vec::new();
    let Some((&count, rest)) = bytes.split_first() else {
        return Ok(out);
    };
    let count = usize::from(count);
    if count > MAX_PANELS || rest.len() != count * RECORD {
        return Ok(out);
    }
    fallible::reserve_exact(&mut out, count)?;
    for record in rest.as_chunks::<RECORD>().0 {
        let at = |i: usize| u16::from_le_bytes([record[i], record[i + 1]]);
        out.push(Panel {
            x: at(0),
            y: at(2),
            w: at(4),
            h: at(6),
        });
    }
    Ok(out)
}

/// A rectangle of the page: columns `x0..x1`, rows `y0..y1`.
#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
}

/// Runs of positions that hold ink, set apart by at least `gap` without.
struct Runs<'a> {
    gap: u32,
    start: Option<u32>,
    last: u32,
    out: &'a mut Vec<(u32, u32)>,
}

impl Runs<'_> {
    fn push(&mut self, at: u32, ink: bool) -> Result<(), DocumentError> {
        if ink {
            if self.start.is_none() {
                self.start = Some(at);
            }
            self.last = at;
        } else if let Some(start) = self.start
            && at - self.last >= self.gap
        {
            fallible::push(self.out, (start, self.last + 1))?;
            self.start = None;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<(), DocumentError> {
        if let Some(start) = self.start.take() {
            fallible::push(self.out, (start, self.last + 1))?;
        }
        Ok(())
    }
}

struct Scan<'a> {
    storage: &'a dyn Storage,
    file: &'a GrayFile,
    rows: Vec<u8>,
    columns: Vec<u16>,
    gap: u32,
}

/// A write that failed.
fn io() -> DocumentError {
    DocumentError::Storage(paperpaw_sdk::StorageError::Io)
}

/// The page is not read.
fn unreadable() -> DocumentError {
    DocumentError::Malformed(alloc::borrow::Cow::Borrowed("page unreadable"))
}

impl<'a> Scan<'a> {
    /// A scan of `file` (a picture of a size [`find`] accepts): its buffers.
    fn new(storage: &'a dyn Storage, file: &'a GrayFile) -> Result<Self, DocumentError> {
        let width = file.width as usize;
        let mut rows = fallible::zeroed(READ.max(width))?;
        // Whole rows only.
        rows.truncate(rows.len() / width * width);
        let mut columns = Vec::new();
        fallible::reserve_exact(&mut columns, width)?;
        columns.resize(width, 0);
        Ok(Self {
            storage,
            file,
            rows,
            columns,
            gap: (file.width.min(file.height) / 100).max(3),
        })
    }

    /// The stretches of `rect` with ink, between rows (`across` rows) or
    /// columns without; at most one more than [`MAX_PANELS`].
    fn cut(
        &mut self,
        rect: Rect,
        across_rows: bool,
        out: &mut Vec<(u32, u32)>,
    ) -> Result<(), DocumentError> {
        let width = self.file.width as usize;
        let per = self.rows.len() / width;
        let span = (rect.x1 - rect.x0) as usize;
        let height = rect.y1 - rect.y0;
        let (low, high) = if across_rows {
            (rect.y0, rect.y1)
        } else {
            (rect.x0, rect.x1)
        };
        let mut runs = Runs {
            gap: self.gap,
            start: None,
            last: 0,
            out,
        };
        if !across_rows {
            self.columns[rect.x0 as usize..rect.x1 as usize].fill(0);
        }
        let mut y = rect.y0;
        while y < rect.y1 {
            let want = per.min((rect.y1 - y) as usize);
            let got = self
                .file
                .read_rows(self.storage, y, &mut self.rows[..want * width]);
            if got == 0 {
                return Err(unreadable());
            }
            for i in 0..got as usize {
                let row = &self.rows[i * width + rect.x0 as usize..][..span];
                if across_rows {
                    let ink = row.iter().filter(|&&p| p < PAPER).count();
                    runs.push(y + i as u32, ink * 64 > span)?;
                } else {
                    let columns = &mut self.columns[rect.x0 as usize..rect.x1 as usize];
                    for (count, &p) in columns.iter_mut().zip(row) {
                        *count += u16::from(p < PAPER);
                    }
                }
            }
            y += got;
        }
        if !across_rows {
            let limit = height as usize / 64;
            for x in low..high {
                let ink = usize::from(self.columns[x as usize]) > limit;
                runs.push(x, ink)?;
            }
        }
        runs.finish()
    }
}

/// A rectangle waiting to be cut: `across_rows` says how, `stuck` that the
/// other way found nothing to cut in it.
struct Item {
    rect: Rect,
    across_rows: bool,
    stuck: bool,
    depth: u8,
}

/// The panels of the page in `file`, in reading order (columns right to
/// left where `rtl`), or none where the page is not cut into at least two,
/// has more than [`MAX_PANELS`], or cannot be read. A short heap is
/// `OutOfMemory`.
pub(crate) fn find(
    storage: &dyn Storage,
    file: &GrayFile,
    rtl: bool,
) -> Result<Vec<Panel>, DocumentError> {
    let (width, height) = (file.width, file.height);
    if width < MIN_SIDE || height < MIN_SIDE || width > MAX_SIDE || height > MAX_SIDE {
        return Ok(Vec::new());
    }
    let mut scan = Scan::new(storage, file)?;
    let whole = Rect {
        x0: 0,
        y0: 0,
        x1: width,
        y1: height,
    };
    let mut waiting: Vec<Item> = Vec::new();
    fallible::push(
        &mut waiting,
        Item {
            rect: whole,
            across_rows: true,
            stuck: false,
            depth: 0,
        },
    )?;
    let mut found: Vec<Rect> = Vec::new();
    let mut parts: Vec<(u32, u32)> = Vec::new();
    let mut cuts = 0;
    while let Some(item) = waiting.pop() {
        cuts += 1;
        if cuts > CUTS || waiting.len() > WAITING || found.len() > MAX_PANELS * 4 {
            return Ok(Vec::new());
        }
        parts.clear();
        if scan.cut(item.rect, item.across_rows, &mut parts).is_err() {
            return Ok(Vec::new());
        }
        let rect_of = |(a, b): (u32, u32)| {
            if item.across_rows {
                Rect {
                    y0: a,
                    y1: b,
                    ..item.rect
                }
            } else {
                Rect {
                    x0: a,
                    x1: b,
                    ..item.rect
                }
            }
        };
        match parts.len() {
            // Nothing but paper.
            0 => {}
            1 => {
                let rect = rect_of(parts[0]);
                if item.stuck || item.depth >= DEPTH {
                    fallible::push(&mut found, rect)?;
                } else {
                    fallible::push(
                        &mut waiting,
                        Item {
                            rect,
                            across_rows: !item.across_rows,
                            stuck: true,
                            depth: item.depth + 1,
                        },
                    )?;
                }
            }
            _ => {
                if parts.len() > MAX_PANELS {
                    return Ok(Vec::new());
                }
                // The first to be read goes in last: the list is popped from
                // its end.
                let backwards = item.across_rows || !rtl;
                for i in 0..parts.len() {
                    let part = if backwards {
                        parts[parts.len() - 1 - i]
                    } else {
                        parts[i]
                    };
                    fallible::push(
                        &mut waiting,
                        Item {
                            rect: rect_of(part),
                            across_rows: !item.across_rows,
                            stuck: false,
                            depth: item.depth + 1,
                        },
                    )?;
                }
            }
        }
    }
    keep(found, (width, height))
}

/// The rectangles that are panels (specks and rules are not), as panels: none
/// where they are fewer than two or more than [`MAX_PANELS`].
fn keep(found: Vec<Rect>, (width, height): (u32, u32)) -> Result<Vec<Panel>, DocumentError> {
    let area = u64::from(width) * u64::from(height);
    let mut panels = Vec::new();
    fallible::reserve_exact(&mut panels, found.len().min(MAX_PANELS))?;
    for r in found {
        let (w, h) = (r.x1 - r.x0, r.y1 - r.y0);
        // Specks and rules are no panels.
        if w < width / 12 || h < height / 12 || u64::from(w) * u64::from(h) * 50 < area {
            continue;
        }
        if panels.len() == MAX_PANELS {
            return Ok(Vec::new());
        }
        panels.push(Panel {
            x: r.x0 as u16,
            y: r.y0 as u16,
            w: w as u16,
            h: h as u16,
        });
    }
    Ok(if panels.len() < 2 { Vec::new() } else { panels })
}

/// Reads `buf` from the picture at `path` (`PPGI`, rows of `width`): row `y`
/// from column `x` on.
fn read_span(
    storage: &dyn Storage,
    file: &GrayFile,
    (x, y): (u32, u32),
    buf: &mut [u8],
) -> Result<(), DocumentError> {
    let at = 12 + u64::from(y) * u64::from(file.width) + u64::from(x);
    let mut filled = 0;
    while filled < buf.len() {
        match storage.read_at(&file.path, at + filled as u64, &mut buf[filled..]) {
            Ok(0) | Err(_) => return Err(unreadable()),
            Ok(n) => filled += n,
        }
    }
    Ok(())
}

/// `panel` of the page in `file`, enlarged as far as the page's own size
/// allows (smooth, its proportions kept) and centred on white, written to
/// `path` as a gray picture of the page's size, through a temporary name.
pub(crate) fn crop(
    storage: &dyn Storage,
    file: &GrayFile,
    panel: Panel,
    path: &str,
) -> Result<(), DocumentError> {
    let (pw, ph) = (u32::from(panel.w), u32::from(panel.h));
    let (x, y) = (u32::from(panel.x), u32::from(panel.y));
    if pw == 0 || ph == 0 || x + pw > file.width || y + ph > file.height {
        return Err(unreadable());
    }
    let (width, height) = (file.width, file.height);
    // 16.16 enlargement, the same both ways.
    let scale =
        ((u64::from(width) << 16) / u64::from(pw)).min((u64::from(height) << 16) / u64::from(ph));
    let scale = scale.max(1 << 16);
    let (cw, ch) = (
        ((u64::from(pw) * scale) >> 16).min(u64::from(width)) as u32,
        ((u64::from(ph) * scale) >> 16).min(u64::from(height)) as u32,
    );
    let (ox, oy) = ((width - cw) / 2, (height - ch) / 2);
    let mut columns: Vec<u32> = Vec::new();
    fallible::reserve_exact(&mut columns, cw as usize)?;
    for c in 0..cw {
        // A position in the panel, 16.16, the last column the panel's own.
        columns.push((((u64::from(c) << 16) * 65536) / scale).min(u64::from(pw - 1) << 16) as u32);
    }
    let mut top = fallible::zeroed(pw as usize)?;
    let mut bottom = fallible::zeroed(pw as usize)?;
    let mut line = fallible::zeroed(width as usize)?;
    let temporary = fallible::try_format!("{path}.tmp")?;
    let result = (|| {
        let mut writer = GrayWriter::new(storage, &temporary);
        writer.start(width, height).map_err(|_| io())?;
        let mut loaded: Option<u32> = None;
        for row in 0..height {
            line.fill(255);
            if row >= oy && row < oy + ch {
                let position =
                    (((u64::from(row - oy) << 16) * 65536) / scale).min(u64::from(ph - 1) << 16);
                let (upper, fraction) = ((position >> 16) as u32, ((position >> 8) & 0xff) as u32);
                let lower = (upper + 1).min(ph - 1);
                match loaded {
                    Some(have) if have == upper => {}
                    Some(have) if have + 1 == upper => {
                        core::mem::swap(&mut top, &mut bottom);
                        read_span(storage, file, (x, y + lower), &mut bottom)?;
                    }
                    _ => {
                        read_span(storage, file, (x, y + upper), &mut top)?;
                        read_span(storage, file, (x, y + lower), &mut bottom)?;
                    }
                }
                loaded = Some(upper);
                for (c, &at) in columns.iter().enumerate() {
                    let (left, mix) = ((at >> 16) as usize, (at >> 8) & 0xff);
                    let right = (left + 1).min(pw as usize - 1);
                    let blend = |a: u8, b: u8| u32::from(a) * (256 - mix) + u32::from(b) * mix;
                    let above = blend(top[left], top[right]);
                    let below = blend(bottom[left], bottom[right]);
                    line[ox as usize + c] =
                        ((above * (256 - fraction) + below * fraction) >> 16) as u8;
                }
            }
            writer.row(&line).map_err(|_| io())?;
        }
        writer.finish().map_err(|_| io())?;
        storage.rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = storage.remove(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use paperpaw_sdk::MemoryStorage;

    fn gray(width: u32, height: u32, data: &[u8]) -> (MemoryStorage, GrayFile) {
        let mut bytes = b"PPGI".to_vec();
        bytes.extend_from_slice(&width.to_le_bytes());
        bytes.extend_from_slice(&height.to_le_bytes());
        bytes.extend_from_slice(data);
        let storage = MemoryStorage::new().with("g", bytes);
        let file = GrayFile {
            path: String::from("g"),
            width,
            height,
        };
        (storage, file)
    }

    #[test]
    fn lists_round_trip_and_reject_lies() {
        let panels = [
            Panel {
                x: 1,
                y: 2,
                w: 3,
                h: 4,
            },
            Panel {
                x: 5,
                y: 6,
                w: 7,
                h: 8,
            },
        ];
        assert_eq!(decode(&encode(&panels).unwrap()).unwrap(), panels);
        assert!(decode(&[]).unwrap().is_empty());
        assert!(decode(&[2, 0, 0]).unwrap().is_empty());
        assert!(decode(&[200]).unwrap().is_empty());
    }

    #[test]
    fn pictures_that_lie_or_are_empty_have_no_panels() {
        // Cut short: the rows are not there.
        let (storage, file) = gray(100, 100, &[255; 500]);
        assert!(find(&storage, &file, false).unwrap().is_empty());
        // Blank, solid, noise, too small, too large.
        let (storage, file) = gray(100, 100, &[255; 10_000]);
        assert!(find(&storage, &file, false).unwrap().is_empty());
        let (storage, file) = gray(100, 100, &[0; 10_000]);
        assert!(find(&storage, &file, false).unwrap().is_empty());
        let noise: Vec<u8> = (0..10_000_u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 24) as u8)
            .collect();
        let (storage, file) = gray(100, 100, &noise);
        assert!(find(&storage, &file, true).unwrap().is_empty());
        let (storage, file) = gray(10, 10, &[0; 100]);
        assert!(find(&storage, &file, false).unwrap().is_empty());
        let (storage, file) = gray(100_000, 100_000, &[]);
        assert!(find(&storage, &file, false).unwrap().is_empty());
        // Stripes of every width: more than the most panels is none.
        let mut stripes = Vec::new();
        for y in 0..400_u32 {
            stripes.extend(core::iter::repeat_n(
                if y / 10 % 2 == 0 { 0 } else { 255 },
                100,
            ));
        }
        let (storage, file) = gray(100, 400, &stripes);
        assert!(find(&storage, &file, false).unwrap().len() <= MAX_PANELS);
    }
}
