//! Mock screens for a simple launcher plugin (#236): four concepts with
//! three screens each, drawn with the SDK's blocks at the reader's text size.
//! The content is made up (`data`); the up and down keys move the focus,
//! nothing opens. Each screen is chosen as the home screen from the command
//! line (`--set slots.core.home=launcher-mocks.<screen>`, see `shots.sh`).
#![no_std]
#![no_main]

extern crate alloc;

#[path = "../../../tools/plugin-host-libs/mem.rs"]
mod mem;

mod book;
mod data;
mod kit;
mod list;
mod panel;
mod tiles;

use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, RedrawMode},
        declare::{Declarations, DeclareError, ScreenDecl},
        events::Incoming,
        fonts::{Font, FontRole},
        image::ImageStyle,
        registrations,
        reply::Reply,
        settings::{self, SettingValue},
        snapshot,
    },
};
use paperpaw_ui_kit::{Style, icons, paint};

use kit::Ui;

/// A screen's drawing.
type Screen = fn(&mut Ui<'_, '_>, usize) -> Result<(), Error>;

/// The screens: the name, how many parts the focus visits, the drawing. A
/// screen's token is its place here, from one.
const SCREENS: [(&str, usize, Screen); 12] = [
    ("a-home", 5, list::home),
    ("a-library", 5, list::library),
    ("a-quick", 6, list::quick),
    ("b-home", 5, tiles::home),
    ("b-library", 4, tiles::library),
    ("b-quick", 6, tiles::quick),
    ("c-home", 2, book::home),
    ("c-shelf", 4, book::shelf),
    ("c-more", 5, book::more),
    ("d-home", 6, panel::home),
    ("d-panel", 12, panel::panel),
    ("d-library", 7, panel::library),
];

struct Mocks {
    focus: [usize; SCREENS.len()],
}

fn declare(host: &Host<'_>) -> Result<(), DeclareError> {
    let mut declarations =
        Declarations::new(|batch| registrations::submit(host, batch).map_err(i32::from));
    for (index, (name, _, _)) in SCREENS.iter().enumerate() {
        declarations.screen(ScreenDecl::new(
            name,
            "launcher-mocks.title",
            index as u32 + 1,
        ))?;
    }
    declarations.submit()
}

impl Plugin for Mocks {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        declare(host).map_err(DeclareError::to_error)?;
        Ok(Self {
            focus: [0; SCREENS.len()],
        })
    }

    fn event(
        &mut self,
        _: &Host<'_>,
        _: u32,
        _: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let at = (token as usize).checked_sub(1).ok_or(Error::Invalid)?;
        let (_, _, screen) = SCREENS.get(at).ok_or(Error::Invalid)?;
        let display = snapshot::capture_snapshot(host)?;
        let mut scratch = [0; 64];
        let de = matches!(
            settings::read_setting(host, "core.language", &mut scratch),
            Ok(SettingValue::Text(language)) if language.starts_with("de")
        );
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let icon_font = icons::font(host)?;
        let drawn = (|| {
            let style = Style::read(host, &display, &font)?;
            // The screen's own part of the display: the row of hints is the host's.
            let pane = kit::B::new(
                frame.rect.x,
                frame.rect.y,
                frame.rect.width as i32,
                frame.rect.height as i32,
            );
            let mut ui = Ui::new(host, &font, icon_font.as_ref(), &style, pane, de);
            ui.cover = kit::sample_cover(host);
            let all = ui.screen;
            ui.fill(all, ui.paper)?;
            screen(&mut ui, self.focus[at])?;
            paint::replay_icons(&mut frame, &font, icon_font.as_ref(), ui.ops.iter())?;
            for (image, place) in &ui.pictures {
                let _ = frame.draw_canvas()?.draw_image(
                    *image,
                    paint::frame_rect(*place),
                    ImageStyle::new(),
                );
            }
            Ok(())
        })();
        if let Some(icon_font) = icon_font {
            icon_font.release()?;
        }
        font.release()?;
        drawn
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Input::Action { action, .. } = input else {
            return Ok(Handled::No);
        };
        let Some(at) = (token as usize)
            .checked_sub(1)
            .filter(|at| *at < SCREENS.len())
        else {
            return Ok(Handled::No);
        };
        let count = SCREENS[at].1;
        let step = match action {
            "core.down" | "core.next" | "core.right" => 1,
            "core.up" | "core.previous" | "core.left" => count - 1,
            // Drawn again as it is: a cover that was being made shows.
            "core.confirm" => 0,
            _ => return Ok(Handled::No),
        };
        self.focus[at] = (self.focus[at] + step) % count;
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)?;
        Ok(Handled::Yes)
    }
}

const CALLS: &[u8] = include_bytes!("../native/metadata.host.bin");

paperpaw_sdk::native_plugin!(Mocks, metadata = CALLS);
