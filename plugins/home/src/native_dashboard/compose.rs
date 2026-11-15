//! Compose only widgets that can contribute pixels to the current band.
use paperpaw_sdk::{Rect, native::Error};

/// Composes each widget into its cell. One the host cannot compose (or that
/// fails to draw) is left out: the rest of the page still shows (as on
/// interface 6). Returns how many were composed. Memory is the exception: a
/// widget that cannot draw for want of it fails the draw, so that the screen is
/// not left with a blank cell nobody sees the reason for.
///
/// # Errors
/// [`Error::NoMemory`] from a widget's draw.
pub(super) fn compose_each<T>(
    widgets: impl Iterator<Item = (T, Rect)>,
    band: (i32, i32),
    mut compose: impl FnMut(T, Rect) -> Result<(), Error>,
) -> Result<usize, Error> {
    let mut composed = 0;
    for (widget, rect) in widgets {
        // The host calls the screen once for each rendered band. Widgets
        // outside it cannot change pixels and must not enter the host again.
        if rect.width == 0
            || rect.height == 0
            || band.0 >= band.1
            || i32::from(rect.y) >= band.1
            || i32::from(rect.y) + i32::from(rect.height) <= band.0
        {
            continue;
        }

        match compose(widget, rect) {
            Ok(()) => composed += 1,
            Err(Error::NoMemory) => return Err(Error::NoMemory),
            Err(_) => {}
        }
    }
    Ok(composed)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;
    #[test]
    fn a_widget_that_fails_to_compose_leaves_the_rest_of_the_page_drawn() {
        let cell = |n: u16| Rect::new(n * 10, 0, 10, 10);
        let mut seen = Vec::new();
        let composed = compose_each(
            [(1_u32, cell(0)), (2, cell(1)), (3, cell(2))].into_iter(),
            (0, 800),
            |id, rect| {
                seen.push((id, rect.x));
                if id == 2 { Err(Error::Denied) } else { Ok(()) }
            },
        )
        .unwrap();
        // All three were tried, in order; the failing one alone is missing.
        assert_eq!(seen, [(1, 0), (2, 10), (3, 20)]);
        assert_eq!(composed, 2);
        // A host that refuses every one: none composed, no error.
        assert_eq!(
            compose_each(
                [(1_u32, cell(0)), (2, cell(1))].into_iter(),
                (0, 800),
                |_, _| Err(Error::Denied)
            ),
            Ok(0)
        );
        // Memory is not a refusal to leave out: the draw fails.
        assert_eq!(
            compose_each(
                [(1_u32, cell(0)), (2, cell(1))].into_iter(),
                (0, 800),
                |_, _| Err(Error::NoMemory)
            ),
            Err(Error::NoMemory)
        );
    }

    #[test]
    fn a_focus_band_does_not_draw_unchanged_widgets_outside_it() {
        // A focus move between tools near the footer leaves the clock, date,
        // continue card and status above the damaged band unchanged.
        let widgets = [
            ("status", Rect::new(35, 8, 410, 36)),
            ("clock", Rect::new(35, 52, 190, 113)),
            ("date", Rect::new(242, 52, 190, 113)),
            ("continue", Rect::new(35, 182, 409, 243)),
            ("target", Rect::new(35, 572, 190, 113)),
        ];
        let mut draws = Vec::new();
        let count = compose_each(widgets.into_iter(), (560, 697), |id, _| {
            draws.push(id);
            Ok(())
        })
        .unwrap();
        assert_eq!(draws, ["target"]);
        assert_eq!(count, 1);
    }

    #[test]
    fn widgets_touching_a_band_edge_are_not_drawn() {
        let widgets = [
            (1, Rect::new(0, 10, 30, 10)),
            (2, Rect::new(0, 20, 30, 10)),
            (3, Rect::new(0, 30, 30, 10)),
        ];
        let mut draws = Vec::new();
        compose_each(widgets.into_iter(), (20, 30), |id, _| {
            draws.push(id);
            Ok(())
        })
        .unwrap();
        assert_eq!(draws, [2]);
    }
}
