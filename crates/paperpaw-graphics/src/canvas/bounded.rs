//! Telling a part within bounds, for parts a host takes from code it does not
//! trust to stop (interface 7's `canvas.describe`, #243): one pass keeps at
//! most so many parts and label bytes, and running out of memory tells
//! nothing instead of aborting.

use super::{Canvas, Described};
use alloc::string::String;

impl Canvas<'_> {
    /// Tells `part` (its area in this canvas's own coordinates) with `label`
    /// to whoever asks, as [`Canvas::describe`] does, but only while the
    /// parts told so far in this pass are fewer than `max_parts` and their
    /// labels with this one hold at most `max_labels` bytes, and only where
    /// memory allows. Returns whether it was told: `false` where nobody asks,
    /// past a bound or out of memory, so a caller may leave out the rest.
    pub fn try_describe(
        &mut self,
        part: Described,
        label: &str,
        max_parts: usize,
        max_labels: usize,
    ) -> bool {
        let (clip, disabled) = (self.clip, self.disabled);
        let area = self.to_screen(part.area);
        let Some(sink) = self.described.as_deref_mut() else {
            return false;
        };
        let held = sink.iter().map(|told| told.label.len()).sum::<usize>();
        if sink.len() >= max_parts || held.saturating_add(label.len()) > max_labels {
            return false;
        }
        let mut text = String::new();
        if text.try_reserve_exact(label.len()).is_err() || sink.try_reserve(1).is_err() {
            return false;
        }
        text.push_str(label);
        sink.push(Described {
            label: text,
            area,
            clip,
            disabled: part.disabled || disabled,
            ..part
        });
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Framebuffer;
    use crate::canvas::Role;
    use alloc::vec::Vec;
    use paperpaw_platform::{PixelFormat, Rect};

    #[test]
    fn parts_stop_at_either_bound_and_where_nobody_asks() {
        let mut fb = Framebuffer::new(32, 32, PixelFormat::Gray4).unwrap();
        let part = || Described::new(Role::Text, "", Rect::new(1, 1, 4, 4));
        let mut canvas = fb.canvas();
        assert!(!canvas.try_describe(part(), "a", 8, 8));
        let mut told = Vec::new();
        {
            let mut canvas = canvas.describing(&mut told);
            assert!(canvas.try_describe(part().focused(true), "abc", 2, 5));
            assert!(
                !canvas.try_describe(part(), "abc", 2, 5),
                "labels past 5 bytes"
            );
            assert!(canvas.try_describe(part(), "de", 2, 5));
            assert!(!canvas.try_describe(part(), "", 2, 5), "parts past 2");
        }
        assert_eq!(told.len(), 2);
        assert_eq!((told[0].label.as_str(), told[0].focused), ("abc", true));
        assert_eq!(told[1].label, "de");
    }
}
