//! The screen under each of the kernel's slots, shown while no plugin
//! provides one. In safe mode it says why the plugins are off and how to
//! start with them again: on an image without built-in screens (an X4 with
//! native plugins only) it is all a safe-mode start shows.
use paperpaw_sdk::builtin::fonts;
use paperpaw_sdk::native::{SafeMode, SafeModeReason};
use paperpaw_sdk::ui::{self, Label, TextRole, UiStyle};
use paperpaw_sdk::widgets::Align;
use paperpaw_sdk::{Canvas, Color, Cx, Env, Handled, Id, Rect, Screen};

/// Its text: "Nothing here yet".
pub(super) const TEXT: &str = "core.screen.placeholder";
/// Its title in safe mode.
pub(super) const SAFE_MODE_TITLE: &str = "core.safe-mode.title";
/// Safe mode after failed starts (or the Power escape): try again.
pub(super) const SAFE_MODE_FAILED: &str = "core.safe-mode.failed";
/// Safe mode the reader asked for by holding the key at power-on.
pub(super) const SAFE_MODE_KEY: &str = "core.safe-mode.key";

/// The text that says why this start is in safe mode; `None` outside it.
pub(super) fn safe_mode_text(safe_mode: Option<&SafeMode>) -> Option<&'static str> {
    Some(match safe_mode?.reason {
        SafeModeReason::KeyHeld => SAFE_MODE_KEY,
        _ => SAFE_MODE_FAILED,
    })
}

/// Shown when no plugin provides a screen for a slot.
#[derive(Debug)]
pub(super) struct PlaceholderScreen;

impl Screen for PlaceholderScreen {
    fn draw(&mut self, canvas: &mut Canvas<'_>, env: &Env<'_>) {
        // The room of the screen, not the canvas's clip: a kernel that
        // draws in bands (the X4) lends each band as the clip.
        let area = room(env);
        let margin = env.metrics().mm(4);
        let line = env.metrics().tenth_mm(5);
        let (w, h) = (u32::from(area.width), u32::from(area.height));
        let inner_w = w.saturating_sub(2 * margin);
        let inner_h = h.saturating_sub(2 * margin);
        canvas.stroke_rect(
            margin as i32,
            margin as i32,
            inner_w,
            inner_h,
            line,
            Color::BLACK,
        );
        let safe_mode = env.native().and_then(|native| native.safe_mode.as_ref());
        if let Some(text) = safe_mode_text(safe_mode) {
            draw_safe_mode(canvas, env, text, margin);
            return;
        }
        if let Some(font) = env.fonts().role(fonts::UI.as_str()) {
            let size = env.metrics().pt(14.0);
            let text = env.i18n().tr(TEXT);
            let width = font.measure(size, text);
            let x = (w as f32 - width) / 2.0;
            let baseline = (h / 2) as i32 + (size / 3.0) as i32;
            font.draw(canvas, size, x, baseline, text, Color::BLACK);
        }
    }

    fn on_action(&mut self, _: &Id, _: &mut Cx<'_, '_>) -> Handled {
        Handled::No
    }
}

/// The room the screen is drawn into, from its own corner.
fn room(env: &Env<'_>) -> Rect {
    let content = env.content();
    Rect::new(0, 0, content.width, content.height)
}

/// The safe-mode title and `text` (a catalog key), wrapped and centred
/// inside the frame. The catalog's texts are borrowed: nothing is kept.
fn draw_safe_mode(canvas: &mut Canvas<'_>, env: &Env<'_>, text: &str, frame: u32) {
    let area = room(env);
    let style = UiStyle::new(env);
    let mut ui = ui::Ui::new(canvas, env, &style);
    let (gap, row) = (style.gap(), style.row());
    let inset = u16::try_from(frame)
        .unwrap_or(u16::MAX)
        .saturating_add(style.margin());
    let inner = area.width.saturating_sub(inset.saturating_mul(2));
    let middle = area.y.saturating_add(area.height / 2);
    let x = area.x.saturating_add(inset);
    Label::new(env.i18n().tr(SAFE_MODE_TITLE))
        .role(TextRole::TITLE)
        .align(Align::Center)
        .draw(
            &mut ui,
            Rect::new(
                x,
                middle.saturating_sub(row.saturating_add(gap)),
                inner,
                row,
            ),
        );
    Label::new(env.i18n().tr(text))
        .align(Align::Center)
        .wrap()
        .draw(&mut ui, Rect::new(x, middle, inner, row.saturating_mul(4)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;
    use alloc::string::String;
    use paperpaw_platform::Storage;
    use paperpaw_sdk::native::Native;
    use paperpaw_sdk::{Plugin, PluginProblem};

    /// The X4's loader in safe mode: no code is loaded.
    struct NoCode;

    impl crate::CodeLoader for NoCode {
        fn load(
            &mut self,
            _: &dyn Storage,
            _: &crate::CodeFile<'_>,
        ) -> Result<Box<dyn Plugin>, PluginProblem> {
            Err(PluginProblem::Unsupported)
        }
    }

    /// What a kernel without any screen of a plugin shows at its start,
    /// all its texts in one.
    fn shown(safe_mode: Option<SafeMode>) -> String {
        let builder = crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(crate::tests::FontPlugin)
            .code_plugins(Native::new("code.ppn"), Box::new(NoCode));
        let mut kernel = match safe_mode {
            Some(safe_mode) => builder.safe_mode(safe_mode),
            None => builder,
        }
        .build()
        .unwrap();
        kernel.start(paperpaw_sdk::Instant(0));
        kernel.render(&mut |_, _| {});
        let mut text = String::new();
        for part in kernel.describe() {
            text.push_str(&part.label);
            text.push(' ');
        }
        text
    }

    #[test]
    fn a_safe_mode_start_with_an_empty_slot_says_why_and_how_to_leave_it() {
        let failed = shown(Some(SafeMode::new(SafeModeReason::FailedStarts)));
        assert!(failed.contains("Safe mode"), "{failed}");
        assert!(failed.contains("after a failed start"), "{failed}");
        assert!(failed.contains("off and on to try again"), "{failed}");
        let held = shown(Some(SafeMode::new(SafeModeReason::KeyHeld)));
        assert!(held.contains("a key was held"), "{held}");
        let normal = shown(None);
        assert!(!normal.contains("Safe mode"), "{normal}");
    }

    /// The gray of each pixel of the frame a kernel started as in `shown`
    /// draws, in bands of `rows` rows or (`None`) all at once.
    fn frame(safe_mode: Option<SafeMode>, rows: Option<u16>) -> alloc::vec::Vec<u8> {
        let mut builder = crate::Kernel::builder(crate::tests::DEVICE)
            .plugin(crate::tests::FontPlugin)
            .code_plugins(Native::new("code.ppn"), Box::new(NoCode));
        if let Some(safe_mode) = safe_mode {
            builder = builder.safe_mode(safe_mode);
        }
        if let Some(rows) = rows {
            builder = builder.frame_rows(rows);
        }
        let mut kernel = builder.build().unwrap();
        kernel.start(paperpaw_sdk::Instant(0));
        let display = crate::tests::DEVICE.display;
        let width = usize::from(display.width);
        let mut frame = alloc::vec![0xff; width * usize::from(display.height)];
        kernel.render(&mut |top, rows| {
            for y in 0..rows.height {
                for x in 0..rows.width {
                    frame[usize::from(top + y) * width + usize::from(x)] = rows.pixel(x, y).g;
                }
            }
        });
        frame
    }

    #[test]
    fn drawn_in_bands_it_is_the_frame_drawn_at_once() {
        for safe_mode in [None, Some(SafeModeReason::FailedStarts)] {
            let whole = frame(safe_mode.map(SafeMode::new), None);
            assert!(whole.iter().any(|gray| *gray < 0x80), "something is drawn");
            for rows in [8, 24, 45] {
                let bands = frame(safe_mode.map(SafeMode::new), Some(rows));
                assert!(bands == whole, "bands of {rows} rows, {safe_mode:?}");
            }
        }
    }

    #[test]
    fn only_safe_mode_has_a_text_and_it_says_why() {
        assert_eq!(safe_mode_text(None), None);
        let text = |reason| safe_mode_text(Some(&SafeMode::new(reason)));
        assert_eq!(text(SafeModeReason::FailedStarts), Some(SAFE_MODE_FAILED));
        assert_eq!(text(SafeModeReason::PowerEscape), Some(SAFE_MODE_FAILED));
        assert_eq!(text(SafeModeReason::KeyHeld), Some(SAFE_MODE_KEY));
    }
}
