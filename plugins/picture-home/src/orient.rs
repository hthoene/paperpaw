//! What a JPEG says about itself without being decoded: its size and the
//! orientation a camera noted (EXIF), found by stepping through its
//! segments with small reads, and the turn a picture needs to be shown
//! upright.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::Storage;
use paperpaw_sdk::image::{GrayFile, GrayWriter, ImageError, ImageSink};

/// The segments looked at at most: a frame comes after a few.
const SEGMENTS: usize = 256;
/// The entries of the first directory of the EXIF block looked at.
const ENTRIES: u64 = 64;
/// The bytes a turn of a picture holds in memory at most.
const TILE: usize = 32 * 1024;

/// Calls `visit` with the marker, where the data begins and how long it is
/// for each segment of the JPEG at `path`, until it returns something or
/// the scan begins.
fn segments<T>(
    storage: &dyn Storage,
    path: &str,
    mut visit: impl FnMut(u8, u64, u64) -> Option<T>,
) -> Option<T> {
    let mut soi = [0u8; 2];
    if storage.read_at(path, 0, &mut soi).ok()? < 2 || soi != [0xff, 0xd8] {
        return None;
    }
    let mut at = 2u64;
    for _ in 0..SEGMENTS {
        let mut head = [0u8; 4];
        if storage.read_at(path, at, &mut head).ok()? < 4 || head[0] != 0xff {
            return None;
        }
        let marker = head[1];
        if marker == 0xff {
            // A fill byte.
            at += 1;
            continue;
        }
        let length = u64::from(u16::from_be_bytes([head[2], head[3]]));
        // The scan or the end: no more of the head to look at.
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
pub(crate) fn jpeg_size(storage: &dyn Storage, path: &str) -> Option<(u32, u32)> {
    segments(storage, path, |marker, data, _| {
        if !matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            return None;
        }
        let mut frame = [0u8; 5];
        if storage.read_at(path, data, &mut frame).ok()? < 5 {
            return Some(None);
        }
        let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
        let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
        Some((width > 0 && height > 0).then_some((width, height)))
    })
    .flatten()
}

/// The orientation (1 to 8) a JPEG's EXIF block notes; 1, as it is, where
/// it notes none.
pub(crate) fn jpeg_orientation(storage: &dyn Storage, path: &str) -> u16 {
    segments(storage, path, |marker, data, length| {
        if marker != 0xe1 || length < 14 {
            return None;
        }
        let mut head = [0u8; 14];
        if storage.read_at(path, data, &mut head).ok()? < 14 || &head[..6] != b"Exif\0\0" {
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
        if directory + 2 > length - 6
            || storage.read_at(path, tiff + directory, &mut count).ok()? < 2
        {
            return Some(1);
        }
        for i in 0..u64::from(word(count)).min(ENTRIES) {
            let mut entry = [0u8; 12];
            let at = tiff + directory + 2 + i * 12;
            if storage.read_at(path, at, &mut entry).ok()? < 12 {
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

/// How a picture as it is stored is turned to be upright: its rows and
/// columns swapped, then mirrored along either side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Turn {
    pub(crate) transpose: bool,
    pub(crate) flip_x: bool,
    pub(crate) flip_y: bool,
}

impl Turn {
    /// The turn of EXIF orientation `orientation` (1 to 8).
    pub(crate) fn of(orientation: u16) -> Self {
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
    pub(crate) fn size(self, (width, height): (u32, u32)) -> (u32, u32) {
        if self.transpose {
            (height, width)
        } else {
            (width, height)
        }
    }
}

/// Writes the transpose of `source` (its columns as rows) to `to`, a tile
/// of columns at a time: a read of a few bytes for each row of the source.
pub(crate) fn transposed(
    storage: &dyn Storage,
    source: &GrayFile,
    to: &str,
) -> Result<GrayFile, ImageError> {
    let (width, height) = (source.width as usize, source.height as usize);
    let columns = (TILE / height.max(1)).clamp(1, width);
    let mut tile: Vec<u8> = Vec::new();
    tile.try_reserve_exact(columns * height)
        .map_err(|_| ImageError::OutOfMemory)?;
    tile.resize(columns * height, 0);
    let mut across = alloc::vec![0u8; columns];
    let mut writer = GrayWriter::new(storage, to);
    writer.start(source.height, source.width)?;
    let mut first = 0;
    while first < width {
        let count = columns.min(width - first);
        for y in 0..height {
            let at = 12 + y as u64 * width as u64 + first as u64;
            let mut filled = 0;
            while filled < count {
                match storage.read_at(&source.path, at + filled as u64, &mut across[filled..count])
                {
                    Ok(0) | Err(_) => return Err(ImageError::Io),
                    Ok(n) => filled += n,
                }
            }
            for (c, value) in across[..count].iter().enumerate() {
                tile[c * height + y] = *value;
            }
        }
        for c in 0..count {
            writer.row(&tile[c * height..(c + 1) * height])?;
        }
        first += count;
    }
    writer.finish()?;
    Ok(GrayFile {
        path: String::from(to),
        width: source.height,
        height: source.width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use paperpaw_sdk::MemoryStorage;

    /// A JPEG's head: an EXIF block of `meta` bytes (with the orientation
    /// where `orientation` is not 0), then a frame of 4000 by 3000.
    fn head(meta: usize, orientation: u16, little: bool) -> Vec<u8> {
        let mut exif = Vec::new();
        exif.extend(b"Exif\0\0");
        exif.extend(if little { *b"II" } else { *b"MM" });
        let word = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let long = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        exif.extend(word(42));
        exif.extend(long(8));
        let entries = if orientation == 0 { 1 } else { 2 };
        exif.extend(word(entries));
        // Another tag first: the orientation is not the first entry.
        exif.extend(word(0x010f));
        exif.extend(word(2));
        exif.extend(long(1));
        exif.extend([0u8; 4]);
        if orientation != 0 {
            exif.extend(word(0x0112));
            exif.extend(word(3));
            exif.extend(long(1));
            exif.extend(word(orientation));
            exif.extend([0u8; 2]);
        }
        exif.resize(exif.len().max(meta), 0);
        let mut bytes = alloc::vec![0xff, 0xd8, 0xff, 0xe1];
        bytes.extend((exif.len() as u16 + 2).to_be_bytes());
        bytes.extend(exif);
        bytes.extend([0xff, 0xc0, 0, 17, 8, 0x0b, 0xb8, 0x0f, 0xa0, 3]);
        bytes.extend([0u8; 9]);
        bytes
    }

    fn storage(bytes: &[u8]) -> MemoryStorage {
        MemoryStorage::new().with("a.jpg", bytes)
    }

    #[test]
    fn the_size_of_a_jpeg_is_found_behind_a_large_block_of_metadata() {
        assert_eq!(
            jpeg_size(&storage(&head(40_000, 0, true)), "a.jpg"),
            Some((4000, 3000))
        );
        assert_eq!(
            jpeg_size(&storage(&head(10, 6, false)), "a.jpg"),
            Some((4000, 3000))
        );
        assert_eq!(jpeg_size(&storage(b"not a picture"), "a.jpg"), None);
        assert_eq!(jpeg_size(&storage(&[0xff, 0xd8, 0xff]), "a.jpg"), None);
    }

    #[test]
    fn the_orientation_is_read_in_either_byte_order() {
        for little in [true, false] {
            for o in 1..=8 {
                assert_eq!(jpeg_orientation(&storage(&head(0, o, little)), "a.jpg"), o);
                assert_eq!(
                    jpeg_orientation(&storage(&head(40_000, o, little)), "a.jpg"),
                    o
                );
            }
            assert_eq!(jpeg_orientation(&storage(&head(0, 0, little)), "a.jpg"), 1);
        }
        assert_eq!(jpeg_orientation(&storage(&head(0, 9, true)), "a.jpg"), 1);
        assert_eq!(jpeg_orientation(&storage(b"junk"), "a.jpg"), 1);
        let mut short = head(0, 6, true);
        short.truncate(20);
        assert_eq!(jpeg_orientation(&storage(&short), "a.jpg"), 1);
    }

    #[test]
    fn the_eight_orientations_turn_as_the_camera_means() {
        // A 3 by 2 picture:  a b c / d e f.
        let stored = [[1u8, 2, 3], [4, 5, 6]];
        let shown = |o: u16| {
            let turn = Turn::of(o);
            let (w, h) = turn.size((3, 2));
            let mut rows = Vec::new();
            for y in 0..h as usize {
                let mut row = Vec::new();
                for x in 0..w as usize {
                    let (sx, sy) = (
                        if turn.flip_x { w as usize - 1 - x } else { x },
                        if turn.flip_y { h as usize - 1 - y } else { y },
                    );
                    // After the transpose: t[sy][sx] = stored[sx][sy].
                    row.push(if turn.transpose {
                        stored[sx][sy]
                    } else {
                        stored[sy][sx]
                    });
                }
                rows.push(row);
            }
            rows
        };
        assert_eq!(shown(1), [[1, 2, 3], [4, 5, 6]]);
        assert_eq!(shown(2), [[3, 2, 1], [6, 5, 4]]);
        assert_eq!(shown(3), [[6, 5, 4], [3, 2, 1]]);
        assert_eq!(shown(4), [[4, 5, 6], [1, 2, 3]]);
        assert_eq!(shown(5), [[1, 4], [2, 5], [3, 6]]);
        // Turned a quarter clockwise: the left column becomes the top row.
        assert_eq!(shown(6), [[4, 1], [5, 2], [6, 3]]);
        assert_eq!(shown(7), [[6, 3], [5, 2], [4, 1]]);
        assert_eq!(shown(8), [[3, 6], [2, 5], [1, 4]]);
    }

    #[test]
    fn a_gray_file_is_transposed_in_tiles() {
        let storage = MemoryStorage::new();
        // 70 by 50, more than a tile of columns at a time is held.
        let (w, h) = (70usize, 50usize);
        {
            let mut writer = GrayWriter::new(&storage, "s.gray");
            writer.start(w as u32, h as u32).unwrap();
            for y in 0..h {
                let row: Vec<u8> = (0..w).map(|x| (x * 3 + y * 7) as u8).collect();
                writer.row(&row).unwrap();
            }
            writer.finish().unwrap();
        }
        let source = GrayFile::open(&storage, "s.gray").unwrap();
        let turned = transposed(&storage, &source, "t.gray").unwrap();
        assert_eq!((turned.width, turned.height), (h as u32, w as u32));
        let opened = GrayFile::open(&storage, "t.gray").unwrap();
        let mut row = alloc::vec![0u8; h];
        for x in [0usize, 1, 33, 69] {
            assert!(opened.read_row(&storage, x as u32, &mut row));
            for (y, v) in row.iter().enumerate() {
                assert_eq!(*v, (x * 3 + y * 7) as u8, "{x},{y}");
            }
        }
    }
}
