//! Where the hints go: each zone's frame and its text's box, and where the
//! box of the others goes. No host: the measuring of text is a closure.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Rect, native::Error};

/// One hint with a zone: its frame, its text and the box the text is in.
#[derive(Debug)]
pub struct ZoneHint {
    pub frame: Rect,
    pub text: String,
    pub label: Rect,
}

/// A zone in thousandths of the screen: x, y, width, height.
pub type Zone = (u16, u16, u16, u16);

/// A zone in thousandths of the screen as pixels on `screen`.
#[must_use]
pub fn zone_on(zone: Zone, screen: Rect) -> Rect {
    let part = |value: u16, size: u16| (u32::from(value.min(1000)) * u32::from(size) / 1000) as u16;
    let (x, y) = (part(zone.0, screen.width), part(zone.1, screen.height));
    Rect::new(
        screen.x.saturating_add(x),
        screen.y.saturating_add(y),
        part(zone.2, screen.width).min(screen.width - x),
        part(zone.3, screen.height).min(screen.height - y),
    )
}

/// The zones' frames and their texts' boxes. `lines` says how many lines a
/// text takes in a width; `small` is a line's height.
///
/// # Errors
/// What `lines` refuses with.
pub fn lay_out_zones(
    zones: &mut [ZoneHint],
    display: (u16, u16),
    (gap, margin, small): (u16, u16, u16),
    lines: &mut dyn FnMut(&str, f32) -> Result<usize, Error>,
) -> Result<(), Error> {
    let (dw, dh) = display;
    for hint in zones {
        let zone = hint.frame;
        let zone = Rect::new(
            zone.x.saturating_add(gap),
            zone.y.saturating_add(gap),
            zone.width.saturating_sub(gap.saturating_mul(2)),
            zone.height.saturating_sub(gap.saturating_mul(2)),
        );
        hint.frame = zone;
        // A narrow part (an edge) has its text beside it, on the side with more room.
        let narrow = u32::from(zone.width) * 4 < u32::from(dw);
        let width = if narrow {
            dw / 2
        } else {
            zone.width.saturating_sub(gap)
        };
        let count = lines(
            &hint.text,
            f32::from(width.saturating_sub(gap.saturating_mul(2))),
        )?;
        let height = clamp(
            u64::from(small)
                .saturating_mul(count as u64)
                .saturating_add(u64::from(gap) * 2),
        );
        let x = if !narrow {
            zone.x.saturating_add(gap / 2)
        } else if u32::from(zone.x) + u32::from(zone.width) / 2 < u32::from(dw) / 2 {
            zone.x
                .saturating_add(zone.width)
                .saturating_add(gap)
                .min(dw.saturating_sub(width.saturating_add(margin)))
        } else {
            zone.x.saturating_sub(width.saturating_add(gap)).max(margin)
        };
        let y = zone
            .y
            .saturating_add(zone.height.saturating_sub(height) / 2)
            .min(dh.saturating_sub(height));
        hint.label = Rect::new(x, y, width, height);
    }
    Ok(())
}

fn clamp(value: u64) -> u16 {
    value.min(u64::from(u16::MAX)) as u16
}

/// Where a box `height` high goes: the top of the box of three places
/// (bottom, middle, top) that hides least of the zones, their texts above all.
#[must_use]
pub fn dialog_top(zones: &[ZoneHint], panel: Rect, display_height: u16, margin: u16) -> u16 {
    let room = display_height.saturating_sub(panel.height);
    let covered = |y: u16| -> u64 {
        let moved = Rect::new(panel.x, y, panel.width, panel.height);
        let area = |r: &Rect| {
            moved
                .intersection(r)
                .map_or(0, |o| u64::from(o.width) * u64::from(o.height))
        };
        zones
            .iter()
            .map(|z| area(&z.frame) + 8 * area(&z.label))
            .sum()
    };
    [room.saturating_sub(margin), room / 2, margin.min(room / 2)]
        .into_iter()
        .min_by_key(|y| covered(*y))
        .unwrap_or(room / 2)
}

/// The zones as hints from `(zone, text)` pairs, none laid out yet.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn zone_hints(screen: Rect, hints: &[(Zone, String)]) -> Result<Vec<ZoneHint>, Error> {
    let mut found = Vec::new();
    found
        .try_reserve_exact(hints.len())
        .map_err(|_| Error::NoMemory)?;
    for (zone, text) in hints {
        found.push(ZoneHint {
            frame: zone_on(*zone, screen),
            text: paperpaw_ui_kit::text::copy(text)?,
            label: Rect::new(0, 0, 0, 0),
        });
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect::new(0, 0, 480, 800);

    fn hint(zone: (u16, u16, u16, u16), text: &str) -> ZoneHint {
        ZoneHint {
            frame: zone_on(zone, SCREEN),
            text: String::from(text),
            label: Rect::new(0, 0, 0, 0),
        }
    }

    #[test]
    fn zones_are_thousandths_of_the_screen_and_stay_on_it() {
        assert_eq!(
            zone_on((500, 0, 500, 500), SCREEN),
            Rect::new(240, 0, 240, 400)
        );
        assert_eq!(
            zone_on((900, 900, 1000, 1000), SCREEN),
            Rect::new(432, 720, 48, 80)
        );
        assert_eq!(
            zone_on((5000, 5000, 5000, 5000), SCREEN),
            Rect::new(480, 800, 0, 0)
        );
    }

    #[test]
    fn a_wide_zone_has_its_text_inside_and_a_narrow_one_beside_it() {
        let mut zones = [
            hint((0, 0, 1000, 500), "wide"),
            hint((900, 0, 100, 1000), "edge"),
        ];
        let mut one = |_: &str, _: f32| Ok(2);
        lay_out_zones(&mut zones, (480, 800), (10, 20, 16), &mut one).unwrap();
        let wide = &zones[0];
        assert_eq!(wide.frame, Rect::new(10, 10, 460, 380));
        assert_eq!(wide.label.x, 15);
        assert_eq!(wide.label.width, 450);
        let edge = &zones[1];
        // At the right edge: the text to the left of its frame.
        assert_eq!(edge.label.width, 240);
        assert!(edge.label.right() <= u32::from(edge.frame.x));
        assert!(edge.label.right() <= 480);
    }

    #[test]
    fn the_box_goes_where_it_hides_least() {
        let zones = [hint((0, 0, 1000, 300), "top")];
        let mut one = |_: &str, _: f32| Ok(1);
        let mut zones = zones;
        lay_out_zones(&mut zones, (480, 800), (10, 20, 16), &mut one).unwrap();
        let panel = Rect::new(20, 0, 440, 200);
        let y = dialog_top(&zones, panel, 800, 20);
        assert_eq!(y, 580, "at the bottom, away from the zone at the top");
    }

    #[test]
    fn absurd_text_heights_and_zones_do_not_overflow() {
        let mut zones = [
            hint((0, 0, 1000, 1000), "x"),
            hint((1000, 1000, 1000, 1000), "y"),
        ];
        let mut many = |_: &str, _: f32| Ok(usize::MAX);
        lay_out_zones(&mut zones, (480, 800), (10, 20, 16), &mut many).unwrap();
        let panel = Rect::new(0, 0, 480, u16::MAX);
        let _ = dialog_top(&zones, panel, 800, 20);
        let mut zero = [hint((0, 0, 0, 0), "z")];
        lay_out_zones(
            &mut zero,
            (0, 0),
            (u16::MAX, u16::MAX, u16::MAX),
            &mut |_, _| Ok(0),
        )
        .unwrap();
    }

    #[test]
    fn building_the_hints_survives_every_refused_allocation() {
        let hints = [
            ((0, 0, 500, 500), String::from("one")),
            ((500, 0, 500, 500), String::from("two")),
        ];
        crate::fail_alloc::sweep(|| zone_hints(SCREEN, &hints).map(|z| z.len()));
    }
}
