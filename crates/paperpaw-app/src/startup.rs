//! A small startup display before the kernel can run: the same UI font,
//! no plugin loading or card access. The shell owns the operation and
//! draws it in bands; normal plugin screens take over after startup.

use paperpaw_core::sdk as paperpaw_sdk;
use paperpaw_sdk::{Canvas, Color, Fonts, Rect, RegisterError};

/// Drawing state for startup, released before the kernel is built.
#[derive(Debug)]
pub struct Screen {
    fonts: Fonts,
}

impl Screen {
    /// The standard UI font, made of the table of built-in fonts as the
    /// kernel makes it; no other font of the table is made.
    pub fn new() -> Result<Self, RegisterError> {
        let font = paperpaw_plugin_fonts_default::DEFAULT_FONTS
            .font(&paperpaw_plugin_fonts_default::SOURCE_SANS)
            .ok_or_else(|| RegisterError::Invalid {
                message: "startup UI font is missing".into(),
            })??;
        let mut fonts = Fonts::new(0);
        fonts.add(font)?;
        Ok(Self { fonts })
    }

    /// Draws the startup operation and its progress. `done == total == 0`
    /// shows a phase without a known total, e.g. verifying a bundle.
    pub fn draw(
        &self,
        canvas: &mut Canvas<'_>,
        screen: Rect,
        phase: &str,
        detail: &str,
        done: u32,
        total: u32,
    ) {
        draw(
            &self.fonts,
            canvas,
            screen,
            [phase, detail, "Please keep the reader on."],
            (done, total),
        );
    }
}

/// Restart feedback using the running kernel's fonts and translated labels.
/// No second font registry is allocated while the kernel is still alive.
pub fn restarting(fonts: &Fonts, canvas: &mut Canvas<'_>, screen: Rect, labels: [&str; 3]) {
    draw(fonts, canvas, screen, labels, (0, 0));
}

fn draw(
    fonts: &Fonts,
    canvas: &mut Canvas<'_>,
    screen: Rect,
    [phase, detail, footer]: [&str; 3],
    (done, total): (u32, u32),
) {
    canvas.clear(Color::WHITE);
    let Some(font) = fonts.get(paperpaw_plugin_fonts_default::SOURCE_SANS.as_str()) else {
        return;
    };
    let width = u32::from(screen.width);
    let x = i32::from(screen.x) + 24;
    let y = i32::from(screen.y) + i32::from(screen.height) / 2;
    font.draw(canvas, 28.0, x as f32, y - 56, "PaperPaw", Color::BLACK);
    font.draw(canvas, 22.0, x as f32, y - 12, phase, Color::BLACK);
    font.draw(canvas, 18.0, x as f32, y + 24, detail, Color::BLACK);
    if total > 0 {
        let bar = width.saturating_sub(48);
        canvas.stroke_rect(x, y + 52, bar, 14, 2, Color::BLACK);
        let filled =
            u64::from(bar.saturating_sub(8)) * u64::from(done.min(total)) / u64::from(total);
        canvas.fill_rect(x + 4, y + 56, filled as u32, 6, Color::BLACK);
    }
    font.draw(canvas, 18.0, x as f32, y + 108, footer, Color::BLACK);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_uses_the_normal_ui_font_without_a_kernel_or_card() {
        let screen = Screen::new().unwrap();
        let font = screen
            .fonts
            .get(paperpaw_plugin_fonts_default::SOURCE_SANS.as_str())
            .unwrap();
        let width = font.measure(22.0, "Plugins could not be installed");
        assert!(
            width > 0.0 && width <= 432.0,
            "fits the X4's portrait screen: {width}"
        );
    }

    #[test]
    fn translated_restart_labels_fit_the_x4_using_existing_fonts() {
        let screen = Screen::new().unwrap();
        let font = screen
            .fonts
            .get(paperpaw_plugin_fonts_default::SOURCE_SANS.as_str())
            .unwrap();
        for catalog in [
            include_str!("../../paperpaw-core/i18n/en.ini"),
            include_str!("../../paperpaw-core/i18n/de.ini"),
        ] {
            let mut count = 0;
            for line in catalog
                .lines()
                .filter(|line| line.starts_with("core.restarting."))
            {
                let (key, text) = line.split_once('=').unwrap();
                let size = if key.trim().ends_with(".title") {
                    22.0
                } else {
                    18.0
                };
                let width = font.measure(size, text.trim());
                assert!(width > 0.0 && width <= 432.0, "{line}: {width}");
                count += 1;
            }
            assert_eq!(count, 3, "all restart labels are translated");
        }
    }
}
