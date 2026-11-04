//! The rectangle writes and the refresh that keeps the grays (#218).

use super::*;

/// Whether the memory `plane` holds white at screen pixel `(x, y)`
/// as `rotation` turns the screen.
fn white_at(plane: &[u8], rotation: Rotation, x: u16, y: u16) -> bool {
    let (c, r) = panel_at(rotation, x, y);
    plane[usize::from(r) * 100 + usize::from(c / 8)] & (0x80 >> (c % 8)) != 0
}

#[test]
fn a_rectangle_goes_in_where_the_band_would_and_nothing_else_changes() {
    // #218: for each way the screen is turned, the part of a band in a
    // rectangle, plain or inverted, lands exactly where the whole band
    // lands, and no byte outside the rectangle's window is touched.
    for rotation in [
        Rotation::Deg0,
        Rotation::Deg90,
        Rotation::Deg180,
        Rotation::Deg270,
    ] {
        let controller = Controller::new();
        let mut display = Display::new(
            Spi(controller.clone()),
            Dc(controller.clone()),
            Pin,
            Busy,
            Delay,
        );
        display.set_rotation(rotation);
        display.init().unwrap();
        let (width, _) = display.size();
        let ink = |x: u16, y: u16| (x * 5 + y * 3).is_multiple_of(7) || x == y;
        let data = band_of(width, 24, ink);
        let rows = FrameView::new(width, 24, PixelFormat::Gray2, usize::from(width) / 4, &data);
        // Rows 40..64 are the band; the rectangle starts and ends off
        // the byte borders of both directions.
        let rect = (13, 45, width.min(100) - 3, 61);
        // The frame as shown: the band, whole, in both memories.
        display.write_rows(40, rows, Planes::BOTH).unwrap();
        let before = controller.borrow().shown.clone();
        display
            .write_rect(40, rows, rect, Planes::SHOWN, true)
            .unwrap();
        let c = controller.borrow();
        assert_eq!(c.new, before, "{rotation:?}: new untouched");
        for y in 40..64 {
            for x in 0..width {
                let inside = x >= rect.0 && x < rect.2 && y >= rect.1 && y < rect.3;
                let want = !ink(x, y - 40) ^ inside;
                assert_eq!(
                    white_at(&c.shown, rotation, x, y),
                    want,
                    "{rotation:?} ({x}, {y})"
                );
            }
        }
        drop(c);
        // Plain: the same as before the inverted one.
        display
            .write_rect(40, rows, rect, Planes::SHOWN, false)
            .unwrap();
        assert!(controller.borrow().shown == before, "{rotation:?}: plain");
        // A rectangle outside the band writes nothing.
        let sent = controller.borrow().commands.len();
        display
            .write_rect(40, rows, (0, 100, width, 120), Planes::BOTH, true)
            .unwrap();
        assert_eq!(controller.borrow().commands.len(), sent, "{rotation:?}");
    }
}

#[test]
fn grays_stay_for_a_fast_refresh_that_asks_for_it_and_only_then() {
    let controller = Controller::new();
    let mut display = Display::new(
        Spi(controller.clone()),
        Dc(controller.clone()),
        Pin,
        Busy,
        Delay,
    );
    display.init().unwrap();
    let data = band(16, |x, y| x == y);
    let rows = view(&data, 16);
    display.write_gray_rows(0, rows).unwrap();
    display.refresh_gray().unwrap();
    // A fast refresh after grays drives every pixel (a half refresh).
    display.refresh(Refresh::Fast, false).unwrap();
    assert_eq!(controller.borrow().refreshes.last().unwrap().1 & 0xF0, 0xD0);
    display.write_gray_rows(0, rows).unwrap();
    display.refresh_gray().unwrap();
    // Asked to keep them: fast, comparing with the frame shown.
    display.start_refresh_keeping_grays(false).unwrap();
    display.wait().unwrap();
    assert_eq!(controller.borrow().refreshes.last().unwrap(), &(0x00, 0xFC));
    // Still gray: the next of the ordinary kind drives every pixel.
    display.refresh(Refresh::Fast, false).unwrap();
    assert_eq!(controller.borrow().refreshes.last().unwrap().1 & 0xF0, 0xD0);
    // And keeping them does nothing special where none are shown.
    display.start_refresh_keeping_grays(false).unwrap();
    display.wait().unwrap();
    assert_eq!(controller.borrow().refreshes.last().unwrap(), &(0x00, 0xFC));
}
