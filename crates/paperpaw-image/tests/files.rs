//! Decoded images on storage (`PPGI` gray, `PPCI` colour): their layout
//! as written byte by byte, what decoders write through the writers
//! reading back as decoded, and files that are damaged, cut short or
//! claim sizes they do not have being refused.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

mod common;

use std::cell::RefCell;
use std::collections::BTreeMap;

use common::{Gif, Jpeg, Png, decoded};
use paperpaw_image::{
    ColorFile, ColorWriter, Fit, Format, GrayFile, GrayWriter, ImageFile, decode,
};
use paperpaw_platform::{DirEntry, Storage, StorageError};

/// Files in memory; reads come back at most `step` bytes at a time.
#[derive(Default)]
struct Files {
    files: RefCell<BTreeMap<String, Vec<u8>>>,
    step: usize,
    /// Appends fail.
    full: bool,
}

impl Files {
    fn put(&self, path: &str, data: &[u8]) {
        self.files.borrow_mut().insert(path.into(), data.to_vec());
    }

    fn get(&self, path: &str) -> Vec<u8> {
        self.files.borrow()[path].clone()
    }
}

impl Storage for Files {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let files = self.files.borrow();
        let data = files.get(path).ok_or(StorageError::NotFound)?;
        let start = (offset as usize).min(data.len());
        let mut n = buf.len().min(data.len() - start);
        if self.step > 0 {
            n = n.min(self.step);
        }
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        let files = self.files.borrow();
        Ok(files.get(path).ok_or(StorageError::NotFound)?.len() as u64)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.put(path, data);
        Ok(())
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        if self.full {
            return Err(StorageError::Full);
        }
        self.files
            .borrow_mut()
            .entry(path.into())
            .or_default()
            .extend_from_slice(data);
        Ok(())
    }
    fn list(&self, _: &str, _: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        Ok(())
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.files.borrow_mut().remove(path);
        Ok(())
    }
}

/// A file of `magic`, `width` and `height` (little endian) and `body`.
fn file(magic: &[u8; 4], width: u32, height: u32, body: &[u8]) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend_from_slice(&width.to_le_bytes());
    out.extend_from_slice(&height.to_le_bytes());
    out.extend_from_slice(body);
    out
}

#[test]
fn gray_files_are_laid_out_as_documented() {
    // `PPGI`, width and height as little-endian u32, rows of gray.
    let storage = Files::default();
    storage.put("a.gray", &file(b"PPGI", 3, 2, &[1, 2, 3, 4, 5, 6]));
    let image = GrayFile::open(&storage, "a.gray").unwrap();
    assert_eq!((image.width, image.height), (3, 2));
    let mut row = [0u8; 3];
    assert!(image.read_row(&storage, 1, &mut row));
    assert_eq!(row, [4, 5, 6]);
    // And so the writer writes them.
    let mut writer = GrayWriter::new(&storage, "b.gray");
    paperpaw_image::ImageSink::start(&mut writer, 3, 2).unwrap();
    paperpaw_image::ImageSink::row(&mut writer, &[1, 2, 3]).unwrap();
    paperpaw_image::ImageSink::row(&mut writer, &[4, 5, 6]).unwrap();
    writer.finish().unwrap();
    assert_eq!(storage.get("b.gray"), storage.get("a.gray"));
}

#[test]
fn colour_files_are_laid_out_as_documented() {
    // `PPCI`, width and height as little-endian u32, rows of RGB.
    let storage = Files::default();
    let body: Vec<u8> = (1..=12).collect();
    storage.put("a.rgb", &file(b"PPCI", 2, 2, &body));
    let image = ColorFile::open(&storage, "a.rgb").unwrap();
    assert_eq!((image.width, image.height), (2, 2));
    let mut rows = [0u8; 12];
    assert_eq!(image.read_rows(&storage, 0, &mut rows), 2);
    assert_eq!(rows.to_vec(), body);
    let both = ImageFile::open(&storage, "a.rgb").unwrap();
    assert!(both.color);
    assert_eq!(
        (both.width, both.height, both.path.as_str()),
        (2, 2, "a.rgb")
    );
}

#[test]
fn files_that_are_not_what_they_claim_are_refused() {
    let storage = Files::default();
    let cases: [(&str, Vec<u8>); 9] = [
        ("empty", Vec::new()),
        ("short header", b"PPGI\x01\x00".to_vec()),
        ("other magic", file(b"PPGX", 1, 1, &[0])),
        ("no width", file(b"PPGI", 0, 5, &[])),
        ("no height", file(b"PPGI", 5, 0, &[])),
        ("a row short", file(b"PPGI", 2, 2, &[0; 3])),
        ("a byte long", file(b"PPGI", 2, 2, &[0; 5])),
        ("huge", file(b"PPGI", u32::MAX, u32::MAX, &[0; 8])),
        ("colour", file(b"PPCI", 1, 1, &[0; 3])),
    ];
    for (what, data) in &cases {
        storage.put("x", data);
        assert_eq!(GrayFile::open(&storage, "x"), None, "{what}");
    }
    assert_eq!(GrayFile::open(&storage, "missing"), None);
    for (what, data) in [
        ("gray", file(b"PPGI", 1, 1, &[0])),
        ("a pixel short", file(b"PPCI", 2, 1, &[0; 5])),
        ("no width", file(b"PPCI", 0, 1, &[])),
    ] {
        storage.put("x", &data);
        assert_eq!(ColorFile::open(&storage, "x"), None, "{what}");
    }
    storage.put("x", b"PPGI");
    assert_eq!(ImageFile::open(&storage, "x"), None);
}

#[test]
fn colour_files_claiming_the_largest_size_are_refused() {
    // The size it checks, 12 + width * height * 3 in u64, overflows for
    // u32::MAX × u32::MAX: a panic in debug builds, for a damaged file
    // on the card (ImageFile::open tries it too).
    let storage = Files::default();
    storage.put("x", &file(b"PPCI", u32::MAX, u32::MAX, &[0; 8]));
    assert_eq!(ColorFile::open(&storage, "x"), None);
    assert_eq!(ImageFile::open(&storage, "x"), None);
}

#[test]
fn rows_past_the_end_are_not_read() {
    let storage = Files::default();
    storage.put("a.gray", &file(b"PPGI", 2, 3, &[1, 2, 3, 4, 5, 6]));
    let image = GrayFile::open(&storage, "a.gray").unwrap();
    let mut out = [0u8; 100];
    // As many whole rows as fit and the image has.
    assert_eq!(image.read_rows(&storage, 0, &mut out[..5]), 2);
    assert_eq!(image.read_rows(&storage, 1, &mut out), 2);
    assert_eq!(&out[..4], [3, 4, 5, 6]);
    assert_eq!(image.read_rows(&storage, 3, &mut out), 0);
    assert_eq!(image.read_rows(&storage, u32::MAX, &mut out), 0);
    assert_eq!(image.read_rows(&storage, 0, &mut out[..1]), 0);
    // A row past the last: the last.
    let mut row = [0u8; 2];
    assert!(image.read_row(&storage, 99, &mut row));
    assert_eq!(row, [5, 6]);
    // A file cut after it was opened: not read.
    storage.put("a.gray", &file(b"PPGI", 2, 3, &[1, 2, 3]));
    assert_eq!(image.read_rows(&storage, 0, &mut out), 0);
    assert!(!image.read_row(&storage, 2, &mut row));

    storage.put("a.rgb", &file(b"PPCI", 1, 2, &[1, 2, 3, 4, 5, 6]));
    let image = ColorFile::open(&storage, "a.rgb").unwrap();
    assert_eq!(image.read_rows(&storage, 1, &mut out), 1);
    assert_eq!(&out[..3], [4, 5, 6]);
    assert_eq!(image.read_rows(&storage, 2, &mut out), 0);
    let mut pixel = [0u8; 3];
    assert!(image.read_row(&storage, u32::MAX, &mut pixel));
    assert_eq!(pixel, [4, 5, 6]);
}

#[test]
fn reads_in_small_pieces_are_put_together() {
    let storage = Files {
        step: 3,
        ..Files::default()
    };
    let body: Vec<u8> = (0..40).collect();
    storage.put("a.gray", &file(b"PPGI", 8, 5, &body));
    let image = GrayFile {
        path: "a.gray".into(),
        width: 8,
        height: 5,
    };
    let mut out = [0u8; 40];
    assert_eq!(image.read_rows(&storage, 0, &mut out), 5);
    assert_eq!(out.to_vec(), body);
    let mut row = [0u8; 8];
    assert!(image.read_row(&storage, 4, &mut row));
    assert_eq!(row.to_vec(), body[32..]);
}

#[test]
fn files_open_on_storage_that_reads_in_pieces() {
    // `Storage::read_at` "reads up to `buf.len()` bytes": GrayFile::open
    // and ColorFile::open read on for their 12-byte header, as the rows'
    // reads do, and do not take a shorter read for a file not theirs.
    let storage = Files {
        step: 5,
        ..Files::default()
    };
    storage.put("a.gray", &file(b"PPGI", 2, 1, &[1, 2]));
    storage.put("a.rgb", &file(b"PPCI", 1, 1, &[1, 2, 3]));
    assert!(GrayFile::open(&storage, "a.gray").is_some());
    assert!(ColorFile::open(&storage, "a.rgb").is_some());
}

#[test]
fn decoded_images_written_read_back_as_decoded() {
    let images = [
        Png::new(37, 29, 6, 8, 1).encode(),
        Jpeg::new(41, 23, &[(2, 2), (1, 1), (1, 1)], 2, true).progressive(),
        Gif::new(19, 33, 16, 3).encode(),
    ];
    for data in &images {
        let format = Format::detect(data).unwrap();
        let fit = Fit::side(30);
        let storage = Files::default();
        // Gray.
        let expected = decoded(data, fit, false);
        let mut writer = GrayWriter::new(&storage, "g");
        let mut source = &data[..];
        decode(format, &mut source, fit, &mut writer).unwrap();
        writer.finish().unwrap();
        let file = GrayFile::open(&storage, "g").unwrap();
        assert_eq!((file.width, file.height), (expected.width, expected.height));
        let mut pixels = vec![0u8; expected.pixels.len()];
        assert_eq!(file.read_rows(&storage, 0, &mut pixels), file.height);
        assert_eq!(pixels, expected.pixels);
        // Colour.
        let expected = decoded(data, fit, true);
        let mut writer = ColorWriter::new(&storage, "c");
        let mut source = &data[..];
        decode(format, &mut source, fit, &mut writer).unwrap();
        writer.finish().unwrap();
        let file = ImageFile::open(&storage, "c").unwrap();
        assert!(file.color);
        let file = ColorFile::open(&storage, "c").unwrap();
        let mut pixels = vec![0u8; expected.pixels.len()];
        assert_eq!(file.read_rows(&storage, 0, &mut pixels), file.height);
        assert_eq!(pixels, expected.pixels);
    }
}

#[test]
fn a_full_card_stops_decoding_with_an_error() {
    // Large enough that rows are written before the end.
    let data = Png::new(300, 200, 0, 8, 5).encode();
    let storage = Files {
        full: true,
        ..Files::default()
    };
    let mut writer = GrayWriter::new(&storage, "g");
    let mut source = &data[..];
    let result = decode(Format::Png, &mut source, Fit::side(1000), &mut writer);
    assert_eq!(result, Err(paperpaw_image::ImageError::Io));
    let mut writer = ColorWriter::new(&storage, "c");
    let mut source = &data[..];
    let result = decode(Format::Png, &mut source, Fit::side(1000), &mut writer);
    assert_eq!(result, Err(paperpaw_image::ImageError::Io));
    // What was written is not taken for a whole image.
    assert_eq!(ImageFile::open(&storage, "g"), None);
    assert_eq!(ImageFile::open(&storage, "c"), None);
}

#[test]
fn try_open_tells_no_image_from_an_image() {
    let storage = Files::default();
    storage.put("a.rgb", &file(b"PPCI", 2, 2, &[7; 12]));
    let opened = ImageFile::try_open(&storage, "a.rgb").unwrap().unwrap();
    assert_eq!((opened.width, opened.height, opened.color), (2, 2, true));
    assert_eq!(opened.path, "a.rgb");
    assert!(matches!(
        paperpaw_image::Decoded::try_open(&storage, "a.rgb"),
        Ok(Some(paperpaw_image::Decoded::Color(_)))
    ));
    // No file, another magic and a size that is not the header's: no image.
    storage.put("short.rgb", &file(b"PPCI", 2, 2, &[7; 11]));
    storage.put("other", &file(b"PPGX", 1, 1, &[0]));
    for path in ["none", "short.rgb", "other"] {
        assert_eq!(ImageFile::try_open(&storage, path), Ok(None), "{path}");
        assert!(ImageFile::open(&storage, path).is_none(), "{path}");
    }
}
