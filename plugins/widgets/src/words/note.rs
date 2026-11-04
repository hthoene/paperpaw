//! A note: a text the reader writes ("If found, please return to …", a
//! reminder), small or large, with a heading of its own if wanted. A tap
//! edits it on the keyboard; so does its setting.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_sdk::ask;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::replies::Typed;
use paperpaw_sdk::setting::{Setting, Text};
use paperpaw_sdk::ui::UiStyle;
use paperpaw_sdk::widgets::{
    self, Align, FixedWidget, Frame, Padding, TextSize, Widget, WidgetTarget,
};
use paperpaw_sdk::{
    Canvas, Command, Env, Id, Rect, RegisterError, Registrar, SettingSpec, SettingValue,
};

use super::frame_height;
use super::text::{self, FACES};

pub(super) const ID: Id = Id::new_static("widgets.note");
/// What the note says.
const TEXT: Setting<Text> = Setting::new("widgets.note.text");
/// A small heading above it, if any.
const HEADING: Setting<Text> = Setting::new("widgets.note.heading");
/// Its font: one of [`FACES`].
const FACE: Setting<Text> = Setting::new("widgets.note.face");
/// The text typed for it on the keyboard.
const TYPED: Event<Typed> = Event::new("widgets.note.typed");

/// The note's settings, kept in the program.
static SETTINGS: [SettingSpec; 9] = [
    SettingSpec::text("widgets.note.text", "widgets.note.setting.text", "", ID)
        .description("widgets.note.setting.text.help"),
    SettingSpec::text(
        "widgets.note.heading",
        "widgets.note.setting.heading",
        "",
        ID,
    )
    .description("widgets.note.setting.heading.help"),
    SettingSpec::choice(
        "widgets.note.face",
        "widgets.setting.font",
        &FACES,
        FACES[0],
        ID,
    )
    .description("widgets.setting.face-font.help"),
    widgets::size_spec("widgets.note.size", ID, TextSize::Fit, true),
    widgets::align_spec("widgets.note.align", ID, Align::Center),
    widgets::frame_spec("widgets.note.frame", ID, Frame::None),
    widgets::padding_spec("widgets.note.padding", ID, Padding::Auto),
    widgets::paper_spec("widgets.note.paper", ID),
    widgets::ink_spec("widgets.note.ink", ID),
];

pub(super) fn register(r: &mut Registrar) -> Result<(), RegisterError> {
    r.add_setting_group({
        static SPEC: paperpaw_sdk::SettingGroupSpec =
            paperpaw_sdk::SettingGroupSpec::new(ID, "widgets.note");
        &SPEC
    })?;
    r.add_settings(&SETTINGS)?;
    r.on(TYPED, |_, typed, out| {
        let text = typed.text.trim();
        out.push(Command::SetSetting(
            TEXT.id().clone(),
            SettingValue::Text(Cow::Owned(String::from(text))),
        ));
    })?;
    static WIDGET: [FixedWidget; 1] =
        [
            FixedWidget::new(ID, "widgets.note", |_| Box::new(Note::default()))
                .category(widgets::categories::PERSONAL),
        ];
    r.add_widgets(&WIDGET)
}

/// The note: its text is a setting, laid out where the widget is placed
/// and when the note changes.
#[derive(Default)]
struct Note {
    /// Where it is drawn, once placed, and the note laid out there.
    placed: Option<(Rect, Option<Laid>)>,
}

/// The note laid out in a widget's area, drawn as it is in every band.
#[derive(Debug, Clone, PartialEq)]
struct Laid {
    /// The heading shows above it.
    heading: bool,
    /// Where its lines go, and how large they are.
    area: Rect,
    size: f32,
    lines: Vec<String>,
    /// Where the first line goes from the top.
    top: u16,
}

/// The note laid out in widget area `area`, as large as its size setting
/// says (or as fits); `None` while it says nothing.
fn lay_out(env: &Env<'_>, area: Rect) -> Option<Laid> {
    let note = env.settings().read(&TEXT).trim();
    if note.is_empty() {
        return None;
    }
    let look = widgets::widget_look(env, ID.as_str(), area);
    let mut area = look.area;
    let style = UiStyle::new(env);
    let font = text::font(env, env.settings().read(&FACE))?;
    // The heading where a line of the note fits below it.
    let least = text::line_height(&font, style.small()) as u16;
    let heading = !env.settings().read(&HEADING).trim().is_empty()
        && area.height >= heading_height(env) + least;
    if heading {
        let height = widgets::caption_height(env).min(area.height);
        area = Rect::new(area.x, area.y + height, area.width, area.height - height);
    }
    let width = f32::from(area.width);
    let size = match widgets::size(env, ID.as_str()) {
        TextSize::Fit => text::largest(style.small(), style.title() * 1.8, |size| {
            // Broken between words only, every word whole on a line.
            let (lines, too_wide) = text::count_lines(&font, size, note, width);
            !too_wide && lines as f32 * text::line_height(&font, size) <= f32::from(area.height)
        }),
        size => size.text(&style),
    };
    let line = text::line_height(&font, size);
    // As many lines as fit; none where not even one has room.
    let fits = (f32::from(area.height) / line) as usize;
    let lines = if fits == 0 {
        Vec::new()
    } else {
        text::lines(&font, size, note, width, fits)
    };
    let total = (line * lines.len() as f32) as u16;
    Some(Laid {
        heading,
        area,
        size,
        lines,
        top: area.y + area.height.saturating_sub(total) / 2,
    })
}

impl Widget for Note {
    /// The note as it is now (its settings changed while shown).
    fn update(&mut self, env: &Env<'_>) -> bool {
        let Some((area, laid)) = &mut self.placed else {
            return false;
        };
        let now = lay_out(env, *area);
        let changed = now != *laid;
        *laid = now;
        changed
    }

    fn place(&mut self, env: &Env<'_>, area: Rect) {
        if self.placed.as_ref().is_none_or(|(at, _)| *at != area) {
            self.placed = Some((area, lay_out(env, area)));
        }
    }

    fn live(&self) -> bool {
        false
    }

    fn height(&self, env: &Env<'_>, width: u16) -> u16 {
        let style = UiStyle::new(env);
        let size = widgets::size(env, ID.as_str()).text(&style);
        let lines = text::font(env, env.settings().read(&FACE)).map_or(1, |font| {
            let width = f32::from(width.saturating_sub(frame_height(env, ID.as_str())));
            text::count_lines(&font, size, env.settings().read(&TEXT), width)
                .0
                .clamp(1, 4)
        });
        let heading = heading_height(env);
        widgets::line_height(env, size) * lines as u16 + heading + frame_height(env, ID.as_str())
    }

    fn draw(&self, canvas: &mut Canvas<'_>, env: &Env<'_>, area: Rect) {
        let look = widgets::draw_frame(canvas, env, ID.as_str(), area);
        let note = env.settings().read(&TEXT).trim();
        if note.is_empty() {
            let key = if env.device().input.touch {
                "widgets.note.none.touch"
            } else {
                "widgets.note.none"
            };
            super::draw_empty(canvas, env, &look, env.i18n().tr(key));
            return;
        }
        let Some(font) = text::font(env, env.settings().read(&FACE)) else {
            return;
        };
        // Laid out when placed; drawn elsewhere, laid out for that.
        let laid = match &self.placed {
            Some((at, Some(laid))) if *at == area => Cow::Borrowed(laid),
            _ => match lay_out(env, area) {
                Some(laid) => Cow::Owned(laid),
                None => return,
            },
        };
        if laid.heading {
            let heading = env.settings().read(&HEADING).trim();
            widgets::draw_caption(canvas, env, ID.as_str(), &look, heading);
        }
        let align = widgets::align(env, ID.as_str());
        let (size, lines) = (laid.size, &laid.lines);
        text::draw_lines(
            canvas, &font, size, lines, laid.area, laid.top, align, look.ink,
        );
    }

    fn targets(&self, env: &Env<'_>, area: Rect) -> Vec<WidgetTarget> {
        let question = ask::text(env.i18n().tr("widgets.note.setting.text"), &TYPED)
            .initial(env.settings().read(&TEXT));
        alloc::vec![WidgetTarget::new(area, question.command())]
    }
}

/// The height of the heading with its gap; 0 without one.
fn heading_height(env: &Env<'_>) -> u16 {
    if env.settings().read(&HEADING).trim().is_empty() {
        0
    } else {
        widgets::caption_height(env)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn its_settings_are_those_read() {
        let ids: alloc::vec::Vec<&str> = super::SETTINGS.iter().map(|s| s.id.as_str()).collect();
        for read in [
            super::TEXT.as_str(),
            super::HEADING.as_str(),
            super::FACE.as_str(),
        ] {
            assert!(ids.contains(&read), "{read}");
        }
        for look in ["size", "align", "frame"] {
            let id = alloc::format!("{}.{look}", super::ID);
            assert!(ids.contains(&id.as_str()), "{id}");
        }
        assert!(super::SETTINGS.iter().all(|s| s.group == super::ID));
    }
}
