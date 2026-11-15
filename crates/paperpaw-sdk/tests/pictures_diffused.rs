//! Pictures diffused into a frame's few grays ([`ui::draw_image`]): the
//! same pixels whether the frame is drawn whole, in bands, each band more
//! than once, or only a part of it again (a notice closing over the
//! picture); colour pictures as their grays.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests panic on purpose when something is wrong"
)]

use std::cell::Cell;

use paperpaw_graphics::Framebuffer;
use paperpaw_image::{ColorWriter, GrayWriter, ImageSink};
use paperpaw_sdk::document::ImageRef;
use paperpaw_sdk::{Color, DirEntry, MemoryStorage, PixelFormat, Rect, Storage, StorageError, ui};

const W: u16 = 120;
const H: u16 = 90;

/// A picture of `width` × `height` with gradients both ways, gray or in
/// colour.
fn picture(storage: &MemoryStorage, width: u32, height: u32, color: bool) -> ImageRef {
    let gray = |x: u32, y: u32| ((x * 255 / (width - 1) + y * 3) % 256) as u8;
    let path = if color { "p.rgb" } else { "p.gray" };
    if color {
        let mut writer = ColorWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).flat_map(|x| [gray(x, y); 3]).collect();
            writer.row_rgb(&row).unwrap();
        }
        writer.finish().unwrap();
    } else {
        let mut writer = GrayWriter::new(storage, path);
        writer.start(width, height).unwrap();
        for y in 0..height {
            let row: Vec<u8> = (0..width).map(|x| gray(x, y)).collect();
            writer.row(&row).unwrap();
        }
        writer.finish().unwrap();
    }
    ImageRef {
        path: path.into(),
        width,
        height,
    }
}

/// Where the picture goes: partly off the frame's top, scaled up.
const AREA: Rect = Rect::new(7, 0, 100, 80);

fn draw(frame: &mut Framebuffer, storage: &MemoryStorage, image: &ImageRef) {
    ui::draw_image(&mut frame.canvas(), storage, image, AREA);
}

#[test]
fn bands_and_parts_drawn_again_show_the_same_pixels() {
    for format in [PixelFormat::Mono1, PixelFormat::Gray2] {
        for color in [false, true] {
            let storage = MemoryStorage::new();
            let image = picture(&storage, 61, 47, color);
            let mut whole = Framebuffer::new(W, H, format).unwrap();
            draw(&mut whole, &storage, &image);

            // In bands of 7 rows, each drawn twice.
            let mut joined = Framebuffer::new(W, H, format).unwrap();
            let mut band = Framebuffer::banded(W, H, 7, format).unwrap();
            for top in (0..H).step_by(7) {
                band.set_top(top);
                for _ in 0..2 {
                    band.canvas().clear(Color::WHITE);
                    draw(&mut band, &storage, &image);
                }
                joined.write_rows(top, band.view());
            }
            assert_eq!(
                whole.view().data,
                joined.view().data,
                "{format:?} {color}: bands"
            );

            // A part drawn again, as where a notice over it closed.
            let part = Rect::new(30, 33, 41, 19);
            let mut again = Framebuffer::new(W, H, format).unwrap();
            draw(&mut again, &storage, &image);
            again.canvas().clipped(part).fill(part, Color::BLACK);
            {
                let mut canvas = again.canvas();
                let mut clipped = canvas.clipped(part);
                clipped.clear(Color::WHITE);
                ui::draw_image(&mut clipped, &storage, &image, AREA);
            }
            assert_eq!(
                whole.view().data,
                again.view().data,
                "{format:?} {color}: a part again"
            );
        }
    }
}

#[test]
fn a_flat_gray_is_diffused_not_a_grid() {
    // A dark gray in black and white: about a third of the pixels white
    // (a little fewer: the diffusion darkens dark grays), and not the
    // same 4 × 4 tile all over, as an ordered dither has it.
    let storage = MemoryStorage::new();
    let mut writer = GrayWriter::new(&storage, "g.gray");
    writer.start(64, 64).unwrap();
    for _ in 0..64 {
        writer.row(&[90; 64]).unwrap();
    }
    writer.finish().unwrap();
    let image = ImageRef {
        path: "g.gray".into(),
        width: 64,
        height: 64,
    };
    let mut frame = Framebuffer::new(64, 64, PixelFormat::Mono1).unwrap();
    ui::draw_image(
        &mut frame.canvas(),
        &storage,
        &image,
        Rect::new(0, 0, 64, 64),
    );
    let white = |x: u16, y: u16| frame.view().pixel(x, y) == Color::WHITE;
    let count = (0..64u16)
        .flat_map(|y| (0..64u16).map(move |x| (x, y)))
        .filter(|&(x, y)| white(x, y))
        .count();
    assert!((1000..=1600).contains(&count), "{count} of 4096 white");
    let tile = |tx: u16, ty: u16| -> Vec<bool> {
        (0..4u16)
            .flat_map(|y| (0..4u16).map(move |x| (x, y)))
            .map(|(x, y)| white(tx * 4 + x, ty * 4 + y))
            .collect()
    };
    let first = tile(0, 0);
    assert!(
        (0..16)
            .flat_map(|ty| (0..16).map(move |tx| (tx, ty)))
            .any(|(tx, ty)| tile(tx, ty) != first)
    );
}

/// The least `scratch_max` (in steps of 64 bytes) with which `picture`
/// is drawn dithered over `area` on a `width` × 4 frame.
fn least_budget(storage: &MemoryStorage, image: &ImageRef, width: u16, area: Rect) -> usize {
    (0..1024)
        .map(|step| step * 64)
        .find(|&budget| {
            let picture = ui::Picture::new(
                &image.path,
                ui::PictureKind::Color,
                image.width,
                image.height,
                budget,
            );
            let mut frame = Framebuffer::new(width, 4, PixelFormat::Mono1).unwrap();
            ui::try_draw_picture(&mut frame.canvas(), storage, &picture, area, 255).is_ok()
        })
        .expect("a budget that draws")
}

#[test]
fn a_dithered_part_falls_back_to_its_visible_columns_where_the_full_width_does_not_fit() {
    let storage = MemoryStorage::new();
    // Enlarged to 1,600 pixels from 1,590: the full width takes a little over
    // 32 KiB of buffers, the 40 columns of a narrow frame far less.
    let image = picture(&storage, 1590, 2, true);
    let area = Rect::new(0, 0, 1600, 4);
    let narrow = least_budget(&storage, &image, 40, area);
    let wide = least_budget(&storage, &image, 1600, area);
    assert!(
        wide > 32 * 1024,
        "{wide}: the full width needs more than the X4 has"
    );
    assert!(narrow < 32 * 1024, "{narrow}: the visible columns fit");
    // So on the X4's cap the part is drawn rather than refused.
    let picture = ui::Picture::new("p.rgb", ui::PictureKind::Color, 1590, 2, 32 * 1024);
    let mut frame = Framebuffer::new(40, 4, PixelFormat::Mono1).unwrap();
    assert_eq!(
        ui::try_draw_picture(&mut frame.canvas(), &storage, &picture, area, 255),
        Ok(())
    );
}

/// A card that counts the bytes read from it.
struct Counted {
    card: MemoryStorage,
    read: Cell<u64>,
}

impl Storage for Counted {
    fn read_at(&self, path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, StorageError> {
        let n = self.card.read_at(path, offset, buf)?;
        self.read.set(self.read.get() + n as u64);
        Ok(n)
    }
    fn size(&self, path: &str) -> Result<u64, StorageError> {
        self.card.size(path)
    }
    fn write(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.write(path, data)
    }
    fn append(&self, path: &str, data: &[u8]) -> Result<(), StorageError> {
        self.card.append(path, data)
    }
    fn list(&self, dir: &str, visit: &mut dyn FnMut(DirEntry<'_>)) -> Result<(), StorageError> {
        self.card.list(dir, visit)
    }
    fn remove(&self, path: &str) -> Result<(), StorageError> {
        self.card.remove(path)
    }
}

/// The frame of the host's draws below, and where their picture goes: made
/// smaller, as a book's cover is.
const FRAME: (u16, u16) = (220, 320);
const PLACE: Rect = Rect::new(10, 12, 200, 300);
/// The most a call reads beyond the rows it draws: its last chunk of rows.
const CHUNK: u64 = 8 * 1024;

/// A card with a gray picture of `width` x `height` on it.
fn card(width: u32, height: u32) -> (Counted, ImageRef) {
    let card = MemoryStorage::new();
    let image = picture(&card, width, height, false);
    let read = Cell::new(0);
    (Counted { card, read }, image)
}

/// The host's draw of a picture ([`ui::try_draw_picture`]) into the band
/// `frame` holds, with the X4's limit on its buffers or `budget`.
fn host_draw(frame: &mut Framebuffer, storage: &Counted, image: &ImageRef, area: Rect) {
    let budget = if image.width > 1000 {
        usize::MAX
    } else {
        32 * 1024
    };
    let kind = ui::PictureKind::Gray;
    let picture = ui::Picture::new(&image.path, kind, image.width, image.height, budget);
    ui::try_draw_picture(&mut frame.canvas(), storage, &picture, area, 255).unwrap();
}

/// A frame of `size` drawn by the host in bands of `rows` rows starting at
/// `tops`, one after the other: its pixels, and the bytes read for it.
fn frame_in_bands(
    storage: &Counted,
    image: &ImageRef,
    (size, area): ((u16, u16), Rect),
    (rows, tops): (u16, &[u16]),
    format: PixelFormat,
) -> (Vec<u8>, u64) {
    let before = storage.read.get();
    let mut joined = Framebuffer::new(size.0, size.1, format).unwrap();
    let mut band = Framebuffer::banded(size.0, size.1, rows, format).unwrap();
    for &top in tops {
        band.set_top(top);
        band.canvas().clear(Color::WHITE);
        host_draw(&mut band, storage, image, area);
        joined.write_rows(top, band.view());
    }
    band.end_frame();
    (joined.view().data.to_vec(), storage.read.get() - before)
}

fn tops(height: u16, rows: u16) -> Vec<u16> {
    (0..height).step_by(usize::from(rows)).collect()
}

/// The host's draw of a dithered picture goes on in each band where the band
/// above ended: the frame shows the same pixels in one band, four, five and
/// nine, and the picture's file is read once for it, not once more for every
/// band above (#250: the X4 read a cover 2.5 times a frame).
#[test]
fn the_hosts_draw_reads_a_picture_once_a_frame_in_any_bands() {
    for format in [PixelFormat::Mono1, PixelFormat::Gray2] {
        let (storage, image) = card(400, 600);
        let whole_frame = (FRAME.1, tops(FRAME.1, FRAME.1));
        let (whole, once) = frame_in_bands(
            &storage,
            &image,
            (FRAME, PLACE),
            (whole_frame.0, &whole_frame.1),
            format,
        );
        assert!(once >= 200_000, "{format:?}: {once} bytes for the picture");
        for rows in [80, 64, 36] {
            let tops = tops(FRAME.1, rows);
            let (banded, read) =
                frame_in_bands(&storage, &image, (FRAME, PLACE), (rows, &tops), format);
            assert_eq!(whole, banded, "{format:?}: bands of {rows} rows");
            let most = once + tops.len() as u64 * CHUNK;
            assert!(
                read <= most,
                "{format:?}: {read} bytes read in bands of {rows} rows, {once} in one"
            );
        }
    }
}

/// A band drawn twice, bands out of their order and a frame drawn once more
/// without its end between (a frame sent again is drawn anew where the kept
/// copy is lost) show the same pixels: the error kept is taken only by the
/// band that begins where it was left, once.
#[test]
fn the_hosts_draw_shows_the_same_in_bands_drawn_again_or_out_of_order() {
    let (storage, image) = card(400, 600);
    let format = PixelFormat::Mono1;
    let all = tops(FRAME.1, FRAME.1);
    let (whole, _) = frame_in_bands(&storage, &image, (FRAME, PLACE), (FRAME.1, &all), format);
    for order in [
        &[0, 0, 64, 64, 128, 192, 256][..],
        &[0, 128, 64, 256, 192][..],
        &[0, 64, 128, 192, 256, 0, 64, 128, 192, 256][..],
        &[256, 192, 128, 64, 0][..],
    ] {
        let (drawn, _) = frame_in_bands(&storage, &image, (FRAME, PLACE), (64, order), format);
        assert_eq!(whole, drawn, "bands at {order:?}");
    }
}

/// A picture too wide for what a frame keeps (4 KiB: 1,024 dithered columns)
/// is dithered from its first row in every band, as before: the same pixels,
/// its rows above read again.
#[test]
fn a_picture_too_wide_to_keep_is_dithered_from_its_first_row_as_before() {
    let (storage, image) = card(1100, 60);
    let (size, area) = ((1100, 60), Rect::new(0, 0, 1100, 60));
    let format = PixelFormat::Mono1;
    let (whole, once) = frame_in_bands(&storage, &image, (size, area), (60, &[0]), format);
    let tops = tops(60, 10);
    let (banded, read) = frame_in_bands(&storage, &image, (size, area), (10, &tops), format);
    assert_eq!(whole, banded);
    assert!(read > 2 * once, "{read} bytes read in bands, {once} in one");
}
