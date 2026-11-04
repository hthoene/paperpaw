//! PaperPaw's logo alone, its name beside it if wanted: the logo as it is set
//! (see [`paperpaw_sdk::logo`]), PaperPaw's own or a picture, read when
//! the widget is placed or the logo set changes (a picture made small
//! for its place then), drawn from memory.

use alloc::boxed::Box;
use alloc::string::{String, ToString};

use paperpaw_sdk::logo::{LOGO, Logo};
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{self, Align, FixedWidget, Frame, Padding, TextSize, Widget};
use paperpaw_sdk::{Canvas, Env, Id, Rect, RegisterError, Registrar, SettingSpec};

use super::frame_height;
use super::text::{self, FACES};

pub(super) const ID: Id = Id::new_static("widgets.logo");
/// The name beside the logo.
const NAME: &str = "name";
/// The name as it is written: a name, not translated.
const PAPERPAW: &str = "PaperPaw";
/// The font of the name: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.logo.face");

/// The logo's settings, kept in the program.
static SETTINGS: [SettingSpec; 8] = [
    widgets::part_spec("widgets.logo.name", ID, "widgets.logo.setting.name", false),
    SettingSpec::choice(
        "widgets.logo.face",
        "widgets.setting.font",
        &FACES,
        FACES[0],
        ID,
    )
    .description("widgets.setting.face-font.help"),
    widgets::size_spec("widgets.logo.size", ID, TextSize::Medium, true),
    widgets::align_spec("widgets.logo.align", ID, Align::Center),
    widgets::frame_spec("widgets.logo.frame", ID, Frame::None),
    widgets::padding_spec("widgets.logo.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.logo.paper", ID),
    widgets::ink_spec("widgets.logo.ink", ID),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.logo");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(ID, "widgets.logo", |_| Box::new(LogoWidget::default()))
                .category(widgets::categories::OTHER),
        ];
    r.add_widgets(&WIDGET)
}

/// The logo, read where it is placed.
#[derive(Default)]
struct LogoWidget {
    /// The logo set ([`LOGO`]) when it was read.
    set: Option<String>,
    /// Where it is drawn, once placed, the logo read for it and a
    /// picture made small for it.
    placed: Option<(Rect, Logo, Option<Thumbnail>)>,
}

impl LogoWidget {
    fn read(&mut self, env: &Env<'_>, area: Rect) {
        let side = side(env, widgets::widget_look(env, ID.as_str(), area).area);
        let logo = Logo::read(env, side, side);
        let small = match &logo {
            Logo::Picture(image) => {
                let (w, h) = logo.fit(side, side);
                Thumbnail::new(env.storage(), image, w, h, env.device().display.format)
            }
            Logo::Drawn => None,
            _ => None,
        };
        self.placed = Some((area, logo, small));
    }
}

impl Widget for LogoWidget {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let set = env.settings().read(&LOGO);
        if self.set.as_deref() == Some(set) {
            return false;
        }
        self.set = Some(set.to_string());
        if let Some((area, _, _)) = self.placed {
            self.read(env, area);
        }
        true
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.placed.as_ref().is_none_or(|(at, _, _)| *at != area) {
            self.read(env, area);
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        height(env, width)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let placed = self
            .placed
            .as_ref()
            .filter(|(at, _, _)| *at == area)
            .map(|(_, logo, small)| (logo, small.as_ref()));
        draw(canvas, env, area, placed);
    }
}

/// The logo's side in `area` (inside the frame).
fn side(env: &Env<'_>, area: Rect) -> u16 {
    sizes(env).1.min(area.height).min(area.width)
}

/// The size of the name, and of the logo: as high as a line and a half
/// of it; as large as its place allows where the size is "fit".
fn sizes(env: &Env<'_>) -> (f32, u16) {
    let chosen = widgets::size(env, ID.as_str());
    let size = chosen.text(&UiStyle::new(env));
    let side = match chosen {
        TextSize::Fit => u16::MAX,
        _ => widgets::line_height(env, size) * 3 / 2,
    };
    (size, side)
}

fn height(env: &Env<'_>, width: u16) -> u16 {
    let side = match widgets::size(env, ID.as_str()) {
        // As high as wide, a third of the screen at most.
        TextSize::Fit => width.min(env.device().display.height / 3),
        _ => sizes(env).1,
    };
    side + frame_height(env, ID.as_str())
}

/// Draws the logo `placed` (read where the widget was placed, a picture
/// made small), or read for `area` now.
fn draw(
    canvas: &mut Canvas<'_>,
    env: &Env<'_>,
    area: Rect,
    placed: Option<(&Logo, Option<&Thumbnail>)>,
) {
    let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
    let area = look.area;
    let size = sizes(env).0;
    let side = side(env, area);
    let read;
    let (logo, small) = match placed {
        Some(placed) => placed,
        None => {
            read = Logo::read(env, side, side);
            (&read, None)
        }
    };
    let (w, h) = logo.fit(side, side);
    // The name a third of the logo's width after it, where it fits.
    let font = text::font(env, env.settings().read(&FACE));
    let gap = w / 3;
    let name = font
        .as_ref()
        .filter(|_| widgets::shows(env, ID.as_str(), NAME));
    let name_width = name.map_or(0.0, |font| font.measure(size, PAPERPAW));
    let with_name = f32::from(w + gap) + name_width <= f32::from(area.width);
    let total = if with_name && name.is_some() {
        f32::from(w + gap) + name_width
    } else {
        f32::from(w)
    };
    let x = widgets::align(env, ID.as_str()).x(area, total);
    let top = area.y + area.height.saturating_sub(h) / 2;
    match small {
        Some(small) if (small.width(), small.height()) == (w, h) => {
            small.draw(canvas, x as u16, top);
        }
        _ => logo.draw(
            canvas,
            env.storage(),
            Rect::new(x as u16, top, w, h),
            look.ink,
        ),
    }
    if let (Some(font), true) = (name, with_name) {
        let row = Rect::new(area.x, top, area.width, h);
        let after = x + f32::from(w + gap);
        ui::draw_label(canvas, font, size, after, row, PAPERPAW, look.ink);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn its_settings_are_in_its_group() {
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&super::FACE.as_str()));
    }
}
