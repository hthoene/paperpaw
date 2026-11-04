//! *Reading cat* (`reading-goal.widget`): the cat of today's mood, as
//! large as its place leaves, and below it the minutes read and the goal
//! (part `minutes`). The picture is made small for its place once, when
//! the mood or the place changes (a [`Thumbnail`] in memory), never read
//! while drawing. A tap opens the goal's setting.

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::semantics::Role;
use paperpaw_sdk::ui::{self, Thumbnail, UiStyle};
use paperpaw_sdk::widgets::{self, CellSpan, Frame, Padding, Widget, WidgetSpec, WidgetTarget};
use paperpaw_sdk::{
    Arg, Canvas, Color, Command, Env, Id, Params, Rect, RegisterError, Registrar, SettingSpec,
};

use crate::{GOAL, Kept, OFFERED, PICTURES, Statistics, Today, pictures};

pub(crate) const WIDGET: Id = Id::new_static(NAME);
const NAME: &str = "reading-goal.widget";

/// The widget's settings, kept in the program.
static SETTINGS: [SettingSpec; 5] = [
    widgets::frame_spec("reading-goal.widget.frame", WIDGET, Frame::None),
    widgets::padding_spec("reading-goal.widget.padding", WIDGET, Padding::Auto),
    widgets::paper_spec("reading-goal.widget.paper", WIDGET),
    widgets::ink_spec("reading-goal.widget.ink", WIDGET),
    widgets::part_spec(
        "reading-goal.widget.minutes",
        WIDGET,
        "reading-goal.part.minutes",
        true,
    ),
];

pub(crate) fn register(r: &mut Registrar, kept: &Rc<Kept>) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(WIDGET, "reading-goal.widget");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    let kept = Rc::clone(kept);
    r.add_widget(
        WidgetSpec::new(WIDGET, "reading-goal.widget", move |_| {
            Box::new(Cat {
                kept: Rc::clone(&kept),
                today: None,
                folder: String::new(),
                area: None,
                picture: None,
            })
        })
        .category(widgets::categories::READING)
        .cells(CellSpan::new(2, 2)),
    )
}

/// "12 / 20 min" ("12 min" without a goal), or what is missing for it.
pub(crate) fn progress(env: &Env<'_>, today: &Today) -> String {
    if today.statistics != Statistics::Counting {
        return env.i18n().tr("reading-goal.uncounted").into();
    }
    let read = ("read", Arg::Int(i64::from(today.minutes)));
    match today.goal {
        Some(goal) => env.i18n().format(
            "reading-goal.progress",
            &[read, ("goal", Arg::Int(i64::from(goal)))],
        ),
        None => env.i18n().format("reading-goal.read", &[read]),
    }
}

/// What a tap does: the goal's setting, or the offer of the statistics
/// where they do not count.
pub(crate) fn tapped(env: &Env<'_>, today: &Today) -> Command {
    let options: &[&str] = match today.statistics {
        Statistics::Counting => {
            let goal = Params::new().with(params::FOCUS, GOAL);
            return Command::OpenSlot(slots::SETTINGS, goal);
        }
        Statistics::Off => &["switch-on", "not-now"],
        Statistics::Missing => &["store", "not-now"],
    };
    paperpaw_sdk::ask::choice(env.i18n().tr("reading-goal.offer"), &OFFERED)
        .options("reading-goal.offer.", options)
        .command()
}

/// `text` in the middle of `room`, on one line.
pub(crate) fn says(
    ui: &mut ui::Ui<'_>,
    font: &paperpaw_sdk::FontHandle<'_>,
    style: &UiStyle,
    text: &str,
    room: Rect,
    color: Color,
) {
    let width = f32::from(room.width);
    let text = ui::fit(font, style.small(), text, width);
    let x = f32::from(room.x) + (width - font.measure(style.small(), &text)).max(0.0) / 2.0;
    let line = font.line_metrics(style.small()).line_height as u16;
    let y = room.y + room.height.saturating_sub(line) / 2;
    ui::Label::new(&text)
        .px(style.small())
        .color(color)
        .draw_from(ui, x, Rect::new(room.x, y, room.width, line));
}

/// What the picture of `today`'s mood says, for screen readers.
pub(crate) fn described(env: &Env<'_>, today: &Today) -> String {
    let key = alloc::format!("reading-goal.mood.{}", today.mood.name());
    env.i18n().tr(&key).into()
}

struct Cat {
    /// What the statistics told last.
    kept: Rc<Kept>,
    today: Option<Today>,
    /// The folder of one's own pictures it was laid out with.
    folder: String,
    area: Option<Rect>,
    /// The picture made small for the area, and where it goes.
    picture: Option<(Thumbnail, u16, u16)>,
}

impl Cat {
    /// The line below the picture, if it shows.
    fn line(env: &Env<'_>) -> u16 {
        if widgets::shows(env, NAME, "minutes") {
            widgets::line_height(env, UiStyle::new(env).small())
        } else {
            0
        }
    }

    /// The picture made small for where the widget is.
    fn lay_out(&mut self, env: &Env<'_>) {
        self.picture = None;
        let (Some(area), Some(today)) = (self.area, self.today) else {
            return;
        };
        let inner = widgets::widget_look(env, NAME, area).area;
        let room = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            inner.height.saturating_sub(Self::line(env)),
        );
        let Some(image) = pictures::of(env, today.mood) else {
            return;
        };
        let (w, h) = ui::fit_image(&image, room.width, room.height);
        if w == 0 || h == 0 {
            return;
        }
        let format = env.device().display.format;
        if let Some(small) = Thumbnail::new(env.storage(), &image, w, h, format) {
            let x = room.x + (room.width - small.width()) / 2;
            let y = room.y + (room.height - small.height()) / 2;
            self.picture = Some((small, x, y));
        }
    }
}

impl Widget for Cat {
    fn update(&mut self, env: &Env<'_>) -> bool {
        let today = Some(Today::now(env, &self.kept));
        let folder = env.settings().text(PICTURES.as_str());
        if today == self.today && folder == self.folder {
            return false;
        }
        let other_picture = self.today.map(|t| t.mood) != today.map(|t| t.mood)
            || folder != self.folder
            || self.picture.is_none();
        self.today = today;
        self.folder = folder.into();
        if other_picture {
            self.lay_out(env);
        }
        true
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.area != Some(area) {
            self.area = Some(area);
            self.lay_out(env);
        }
    }

    fn live(&self) -> bool {
        // Asked each minute: a new day makes the cat sad again.
        true
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        // In a column of widgets (the sleep screen's): a picture about
        // as high as half the width.
        widgets::frame_height(env, NAME) + width / 2 + Self::line(env)
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, NAME, area);
        let Some(today) = &self.today else {
            return;
        };
        let inner = look.area;
        if let Some((picture, x, y)) = &self.picture {
            picture.draw(canvas, *x, *y);
            let at = Rect::new(*x, *y, picture.width(), picture.height());
            canvas.describe(Role::Image, &described(env, today), at);
        }
        let line = Self::line(env);
        let style = UiStyle::new(env);
        let mut ui = ui::Ui::new(canvas, env, &style);
        let Some(font) = ui.font() else {
            return;
        };
        if self.picture.is_none() {
            // The cat's file is not on the card: its mood in words, in the
            // picture's place.
            let room = Rect::new(
                inner.x,
                inner.y,
                inner.width,
                inner.height.saturating_sub(line),
            );
            says(
                &mut ui,
                &font,
                &style,
                &described(env, today),
                room,
                look.ink,
            );
        }
        if line == 0 {
            return;
        }
        let text = progress(env, today);
        let width = f32::from(inner.width);
        let text = ui::fit(&font, style.small(), &text, width);
        let x = f32::from(inner.x) + (width - font.measure(style.small(), &text)).max(0.0) / 2.0;
        let row = Rect::new(inner.x, inner.y + inner.height - line, inner.width, line);
        ui::Label::new(&text)
            .px(style.small())
            .color(look.ink)
            .draw_from(&mut ui, x, row);
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let today = self.today.unwrap_or_else(|| Today::now(env, &self.kept));
        alloc::vec![WidgetTarget::new(area, tapped(env, &today))]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The settings kept in the program are named after the widget, as
    /// it reads them.
    #[test]
    fn settings_are_the_widget_s() {
        for spec in &SETTINGS {
            assert!(
                spec.id.as_str().starts_with("reading-goal.widget."),
                "{}",
                spec.id
            );
            assert_eq!(spec.group.as_str(), NAME);
        }
    }
}
