//! The widget pack on the flat native boundary: the host makes an instance
//! when a screen shows a widget, asks it for its height and where it sits, and
//! calls the plugin to draw it. Nothing of a callback is kept; an instance is
//! the widget's id with its placement and the rectangle it was placed in.
use crate::native_defs::{Tap, behaviour, token_of};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Rect,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::CommandSender,
        events::{AnswerOutcome, Answered, BookKind, Event, Incoming, kinds},
        fonts::{Font, FontRole},
        image::ImageStyle,
        registrations,
        reply::Reply,
        ui::{
            Instance, Open, Param, Question, RequestId, Ui, WidgetInputKind, WidgetTarget,
            stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::{Screens, paint, read_event};

use crate::native_todo_screen::{self, TodoView};
use paperpaw_ui_kit::widget::Cx;

mod image {
    include!(concat!(env!("OUT_DIR"), "/widgets_declarations.rs"));
}

/// The callback token of the minute tick, as declared.
const TICK: u32 = crate::declarations::TICK;
/// The callback token of the reading events, as declared.
const READING: u32 = crate::declarations::READING;
/// The target of a widget whose tap opens its settings.
const SETTINGS_TARGET: u32 = 1;
/// The slot the settings fill.
const SETTINGS_SLOT: &str = "core.settings";
/// The most bytes of a stage reply (sizes and up to 32 targets).
const REPLY: usize = 1280;

#[derive(Debug)]
pub(crate) struct Item {
    pub(crate) instance: Instance,
    pub(crate) token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last drawn or asked about, for the tick.
    shown: Option<u64>,
    /// A cover was still being made when it was drawn: the tick asks for a new
    /// draw until that is queued.
    retry: bool,
}

/// What an open question is for.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Asked {
    /// The text of the setting with this id.
    Setting(String),
    /// A new to-do.
    Add,
    /// New words for to-do `index`.
    Edit { index: usize },
    /// What to do with to-do `index` (worded `title` when asked).
    Options { index: usize, title: String },
}

/// The widget pack.
#[derive(Debug, Default)]
pub struct NativeWidgets {
    pub(crate) items: Vec<Item>,
    /// The question open, and what it is for.
    pub(crate) asked: Option<(RequestId, Asked)>,
    /// The to-do list's screens open.
    pub(crate) screens: Screens<TodoView>,
    /// The to-do list as last read.
    pub(crate) todo: crate::native_todo::Cache,
    /// How often a quote was shown.
    showings: crate::native_quote::Showings,
    /// The highlight chosen, and how often a highlight was shown.
    highlight: crate::native_highlight::Cache,
    highlights: crate::native_quote::Showings,
    /// The library's numbers as last counted.
    numbers: crate::native_numbers::Cache,
    /// The favourites and the series as last read.
    favourites: crate::native_favourites::Cache,
    series: crate::native_series::Cache,
    next: crate::native_next::Cache,
    /// The book being read, for the progress and reading widgets.
    current: crate::native_current::Cache,
    /// The page, chapter and speed of the book read last.
    recorder: crate::native_recorder::Recorder,
}

impl NativeWidgets {
    fn find(&self, instance: u32) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.instance.get() == instance)
    }

    fn made(&mut self, widget: &str, instance: Instance) -> Result<(), Error> {
        let Some(token) = token_of(widget) else {
            return Ok(());
        };
        self.items.retain(|item| item.instance != instance);
        self.items.try_reserve(1).map_err(|_| Error::NoMemory)?;
        self.items.push(Item {
            instance,
            token,
            widget: paperpaw_ui_kit::text::copy(widget)?,
            shown: None,
            retry: false,
        });
        Ok(())
    }

    fn stage(
        &mut self,
        host: &Host<'_>,
        payload: &[u8],
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self
                    .items
                    .iter()
                    .find(|item| item.instance.get() == instance.get())
                    .ok_or(Error::NotFound)?;
                let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let width = u16::try_from(width).unwrap_or(u16::MAX);
                let height = if item.token == crate::native_todo::TOKEN {
                    crate::native_todo::height_with(&mut cx, width, &self.todo)?
                } else if item.token == crate::native_favourites::TOKEN {
                    crate::native_favourites::height_with(&mut cx, width, &mut self.favourites)?
                } else {
                    (behaviour.height)(&mut cx, width)?
                };
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let item = self
                    .items
                    .iter()
                    .find(|item| item.instance.get() == instance.get())
                    .ok_or(Error::NotFound)?;
                let tap = behaviour(item.token).map(|b| b.tap);
                if matches!(tap, Some(Tap::Books | Tap::Next)) {
                    let cx = Cx::open(host, &item.widget)?;
                    let area = frame_to_area(rect);
                    let targets = if item.token == crate::native_reading::TOKEN {
                        crate::native_reading::targets_with(&cx, area, &mut self.current)?
                    } else if item.token == crate::native_progress::TOKEN {
                        crate::native_progress::targets_with(&cx, area, &mut self.current)?
                    } else if item.token == crate::native_next::TOKEN {
                        crate::native_next::targets_with(&cx, area, &mut self.next)?
                    } else if item.token == crate::native_favourites::TOKEN {
                        crate::native_favourites::targets_with(&cx, area, &mut self.favourites)?
                    } else {
                        crate::native_series::targets_with(&cx, area, &mut self.series)?
                    };
                    call.reply_targets(&targets, &mut out)?
                } else if tap == Some(Tap::TickItems) {
                    let cx = Cx::open(host, &item.widget)?;
                    let area = frame_to_area(rect);
                    let targets = crate::native_todo::targets(&cx, area, &self.todo)?;
                    call.reply_targets(&targets, &mut out)?
                } else if tap.is_some_and(|tap| tap != Tap::None) {
                    call.reply_targets(&[WidgetTarget::new(SETTINGS_TARGET, rect)], &mut out)?
                } else {
                    call.reply_targets(&[], &mut out)?
                }
            }
            _ => return Err(Error::Unsupported),
        };
        reply.write(out.get(..written).ok_or(Error::NoSpace)?)
    }

    /// The text typed for a widget's setting: kept as the setting's value.
    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((request, asked)) = self.asked.take() else {
            return Ok(());
        };
        let Asked::Setting(setting) = asked else {
            return self.todo_answered(host, (request, asked), answer);
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let ui = Ui::open(host)?;
        let typed = paperpaw_ui_kit::uitext::read_all(
            &ui,
            host,
            request.text(),
            crate::native_note::TEXT_LIMIT,
        );
        let released = ui.release(host);
        let typed = typed?;
        released?;
        let mut scratch = paperpaw_ui_kit::mem::buffer(2048)?;
        CommandSender::new(host, &mut scratch).set_setting(
            setting.as_str(),
            paperpaw_sdk::native::settings::SettingValue::Text(typed.trim()),
        )
    }

    /// Tells the makers whose widget shows something new since the last minute.
    /// A widget that fails (its time or its command) is left for the next
    /// tick and costs the others nothing: the tick itself is not refused.
    fn tick(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut scratch = [0; 64];
        let mut commands = CommandSender::new(host, &mut scratch);
        for item in &mut self.items {
            let Some(behaviour) = behaviour(item.token) else {
                continue;
            };
            let instance = item.instance;
            let shown = if item.token == crate::native_todo::TOKEN {
                crate::native_todo::shows_with(host, &item.widget, &self.todo)
            } else if item.token == crate::native_quote::TOKEN {
                crate::native_quote::shows_with(host, &item.widget, &self.showings)
            } else if item.token == crate::native_reading::TOKEN {
                crate::native_reading::shows_with(host, &item.widget, &self.recorder)
            } else if item.token == crate::native_highlight::TOKEN {
                crate::native_highlight::shows_with(
                    host,
                    &item.widget,
                    &mut self.highlight,
                    &self.highlights,
                )
            } else {
                (behaviour.shows)(host, &item.widget)
            };
            settle(&mut item.shown, shown, || commands.widget_changed(instance));
            retry(&mut item.retry, || commands.widget_changed(instance));
        }
        Ok(())
    }
}

/// Notes what a widget shows now. If it differs from what it showed before,
/// `queue` tells the maker, and only once that is queued is it remembered as
/// shown; a refusal of either leaves `shown` as it was, so the next tick
/// tries again.
fn settle(
    shown: &mut Option<u64>,
    now: Result<u64, Error>,
    queue: impl FnOnce() -> Result<(), Error>,
) {
    let Ok(now) = now else {
        return;
    };
    if shown.is_some_and(|before| before != now) && queue().is_err() {
        return;
    }
    *shown = Some(now);
}

/// Asks for a new draw while `wanted` (a cover was still being made), and
/// stops asking once that is queued; a refusal leaves it for the next tick.
fn retry(wanted: &mut bool, queue: impl FnOnce() -> Result<(), Error>) {
    if *wanted && queue().is_ok() {
        *wanted = false;
    }
}

impl Plugin for NativeWidgets {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::WIDGETS
            .iter()
            .chain(image::SCREENS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        incoming: Incoming<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let Some(event) = read_event!(incoming; kinds::WidgetMade, kinds::WidgetDropped, kinds::StageCall, kinds::Tick, kinds::Answered, kinds::PageTurned, kinds::BookOpened, kinds::BookClosed, kinds::Suspend, kinds::Stopping)
        else {
            return Ok(());
        };
        match event {
            Event::WidgetMade {
                widget,
                instance,
                now_ms,
                ..
            } => {
                match paperpaw_ui_kit::widget::settings::base(widget) {
                    "widgets.quote" => self.showings.made(now_ms),
                    "widgets.highlight" => self.highlights.made(now_ms),
                    _ => {}
                }
                self.made(widget, instance)
            }
            Event::WidgetDropped { instance, .. } => {
                self.items.retain(|item| item.instance != instance);
                Ok(())
            }
            Event::StageCall {
                stage: "core.widget",
                payload,
                ..
            } => self.stage(host, payload, reply),
            Event::Tick { .. } if token == TICK => self.tick(host),
            Event::PageTurned(view) if token == READING => self.recorder.reading(host, &view, true),
            Event::Book {
                kind: BookKind::Opened,
                reading,
                ..
            } if token == READING => self.recorder.reading(host, &reading, false),
            Event::Book {
                kind: BookKind::Closed,
                reading,
                ..
            } if token == READING => self.recorder.closed(host, &reading),
            Event::Suspend { .. } | Event::Stopping { .. } if token == READING => {
                self.recorder.save(host);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            Event::ActionRun {
                action: native_todo_screen::OPEN_ACTION,
                ..
            } if token == native_todo_screen::OPEN => Self::open_todo_screen(host),
            Event::ScreenOpened { instance, .. } => self.todo_opened(host, instance),
            Event::ScreenShown {
                instance: Some(instance),
                ..
            } => self.screens.shown(native_todo_screen::SCREEN, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn draw(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if frame.purpose == DrawPurpose::Screen {
            return self.todo_draw(host, frame);
        }
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let mut pictures = paperpaw_ui_kit::covers::Pictures::default();
        {
            let item = self
                .items
                .iter()
                .find(|item| item.instance.get() == frame.id)
                .ok_or(Error::NotFound)?;
            let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
            let mut cx = Cx::open(host, &item.widget)?;
            let area = Rect::new(
                paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.x)),
                paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.y)),
                paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.width)),
                paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.height)),
            );
            let mut out = Vec::new();
            if item.token == crate::native_reading::TOKEN {
                let kept = (&mut self.current, &mut pictures, &mut self.recorder);
                crate::native_reading::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_progress::TOKEN {
                let kept = (&mut self.current, &mut pictures);
                crate::native_progress::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_next::TOKEN {
                let kept = (&mut self.next, &mut pictures);
                crate::native_next::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_favourites::TOKEN {
                let kept = (&mut self.favourites, &mut pictures);
                crate::native_favourites::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_series::TOKEN {
                let kept = (&mut self.series, &mut pictures);
                crate::native_series::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_todo::TOKEN {
                crate::native_todo::draw_with(&mut cx, &mut out, area, &self.todo)?;
            } else if item.token == crate::native_quote::TOKEN {
                crate::native_quote::draw_with(&mut cx, &mut out, area, &self.showings)?;
            } else if item.token == crate::native_highlight::TOKEN {
                let kept = (&mut self.highlight, &self.highlights);
                crate::native_highlight::draw_with(&mut cx, &mut out, area, kept)?;
            } else if item.token == crate::native_numbers::TOKEN {
                crate::native_numbers::draw_with(&mut cx, &mut out, area, &mut self.numbers)?;
            } else {
                (behaviour.draw)(&mut cx, &mut out, area)?;
            }
            let bold = if out
                .iter()
                .any(|op| matches!(op, paperpaw_ui_kit::Paint::Bold { .. }))
            {
                Some(Font::acquire_role(host, FontRole::UiBold)?)
            } else {
                None
            };
            paint::replay_with(&mut frame, &cx.font, bold.as_ref(), &out)?;
        }
        // The covers go where nothing else is drawn. One the host no longer has
        // (it ended the id for room) or cannot draw is left out: the next draw
        // asks for it again.
        for (image, at) in &pictures.placed {
            frame
                .draw_canvas()?
                .draw_image(*image, paint::frame_rect(*at), ImageStyle::new().small())
                .ok();
        }
        if pictures.busy {
            // A cover still being made: the next tick draws the widget again.
            let instance = frame.id;
            if let Some(item) = self
                .items
                .iter_mut()
                .find(|item| item.instance.get() == instance)
            {
                item.retry = true;
            }
        }
        Ok(())
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        if token == native_todo_screen::SCREEN {
            return self.todo_input(host, input);
        }
        let Input::Widget {
            instance,
            target,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        let item = self
            .items
            .iter()
            .find(|item| item.instance == instance)
            .ok_or(Error::NotFound)?;
        let base = paperpaw_ui_kit::widget::settings::base(&item.widget);
        let tap = behaviour(item.token).map(|b| b.tap);
        if tap == Some(Tap::TickItems) {
            // The heading, "more" and an empty list open the list's own
            // screen, which is not ported yet: a tap there does nothing.
            if target == crate::native_todo::OPEN_TARGET {
                return Ok(Handled::No);
            }
            // A tick that fails says so, as before, and the widget is drawn
            // again either way (it shows what the card holds).
            if let Err(error) = crate::native_todo::tick(host, target, &self.todo) {
                let key = if error == Error::Unsupported {
                    "widgets.todo.unreadable"
                } else {
                    "widgets.todo.not-saved"
                };
                let _ = notify(host, key);
            }
            self.todo.forget();
            CommandSender::new(host, &mut [0; 64]).widget_changed(item.instance)?;
            return Ok(Handled::Yes);
        }
        if tap == Some(Tap::Next) {
            if target == crate::native_next::LIBRARY_TARGET {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action("core.library")?;
                return Ok(Handled::Yes);
            }
            let book = if item.token == crate::native_progress::TOKEN
                || item.token == crate::native_reading::TOKEN
            {
                crate::native_progress::book(host, &mut self.current)?
            } else {
                crate::native_next::book(host, &item.widget, &mut self.next)?
            };
            let Some(book) = book else {
                return Ok(Handled::No);
            };
            crate::native_books::open_book(host, book)?;
            return Ok(Handled::Yes);
        }
        if tap == Some(Tap::Books) {
            if target & crate::native_shelf::BOOK_TARGET == 0 {
                return Ok(Handled::No);
            }
            let index = usize::try_from(target & !crate::native_shelf::BOOK_TARGET).unwrap_or(0);
            let widget = item.widget.as_str();
            let book = if item.token == crate::native_favourites::TOKEN {
                crate::native_favourites::book_at(host, widget, &mut self.favourites, index)?
            } else {
                crate::native_series::book_at(host, widget, &mut self.series, index)?
            };
            return match book {
                Some(book) => {
                    crate::native_books::open_book(host, book)?;
                    Ok(Handled::Yes)
                }
                None => Ok(Handled::No),
            };
        }
        if target != SETTINGS_TARGET {
            return Ok(Handled::No);
        }
        match tap {
            Some(Tap::Settings) => open_settings(host, base)?,
            Some(Tap::OpenHighlight) => {
                let settings = paperpaw_ui_kit::widget::Settings::new(host, &item.widget);
                let chosen = crate::native_highlight::choose(
                    host,
                    &settings,
                    &mut self.highlight,
                    &self.highlights,
                )?;
                match chosen {
                    Some(quote) => crate::native_highlight::open(host, quote)?,
                    None => return Ok(Handled::No),
                }
            }
            Some(Tap::Run(action)) => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(action)?;
            }
            Some(Tap::TurnQuote) => {
                crate::native_quote::turn(host, base)?;
                CommandSender::new(host, &mut [0; 64]).widget_changed(item.instance)?;
            }
            Some(Tap::Text { key, title }) => {
                // One request at a time: the keyboard of the first is still
                // open, and its answer is the one that counts.
                if self.asked.is_none() {
                    let (request, setting) = ask_text(host, base, key, title)?;
                    self.asked = Some((request, Asked::Setting(setting)));
                }
            }
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }
}

/// Says `key` (a translation key) in a notice.
fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = paperpaw_ui_kit::Messages::new()?.copy(host, key)?;
    let mut scratch = paperpaw_ui_kit::mem::buffer(1024)?;
    CommandSender::new(host, &mut scratch).notify(paperpaw_sdk::native::ui::Notify::new(&text)?)
}

/// The rectangle of a stage call as the plugin's drawing rectangle.
fn frame_to_area(rect: paperpaw_sdk::native::callbacks::FrameRect) -> Rect {
    Rect::new(
        paperpaw_ui_kit::geometry::clamp_u16(i64::from(rect.x)),
        paperpaw_ui_kit::geometry::clamp_u16(i64::from(rect.y)),
        paperpaw_ui_kit::geometry::clamp_u16(i64::from(rect.width)),
        paperpaw_ui_kit::geometry::clamp_u16(i64::from(rect.height)),
    )
}

/// Asks for the text of the setting `<widget>.<key>` on the keyboard, the
/// setting's text to start with; the request and the setting's id.
fn ask_text(
    host: &Host<'_>,
    widget: &str,
    key: &str,
    title: &str,
) -> Result<(RequestId, String), Error> {
    let setting = paperpaw_ui_kit::widget::Key::join(widget, key)?;
    let current = paperpaw_ui_kit::widget::Settings::new(host, widget)
        .string(key, crate::native_note::TEXT_LIMIT)?;
    let mut messages = paperpaw_ui_kit::Messages::new()?;
    let title = messages.copy(host, title)?;
    let ui = Ui::open(host)?;
    let made = (|| {
        let mut input = paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?;
        let mut out = paperpaw_ui_kit::mem::buffer(paperpaw_ui_kit::mem::SERVICE_BUFFER)?;
        let request = ui.ask(host, Question::text(&title, "")?, &mut input, &mut out)?;
        // The text so far, as the initial text (in parts, as the host holds
        // them); nothing to write for an empty one.
        if !current.is_empty() {
            paperpaw_ui_kit::uitext::write_all(&ui, host, request.text(), &current)?;
        }
        Ok::<_, Error>(request)
    })();
    let released = ui.release(host);
    let request = made?;
    released?;
    Ok((request, paperpaw_ui_kit::text::copy(setting.as_str())?))
}

/// Opens the settings on the page of the widget `widget` (its id without the
/// placement).
fn open_settings(host: &Host<'_>, widget: &str) -> Result<(), Error> {
    let ui = Ui::open(host)?;
    let made = (|| {
        let mut input = paperpaw_ui_kit::mem::buffer(1024)?;
        let mut out = paperpaw_ui_kit::mem::buffer(1024)?;
        ui.handover(host, &[Param::new("id", widget)?], &mut input, &mut out)
    })();
    let released = ui.release(host);
    let handover = made?;
    released?;
    let mut scratch = [0; 256];
    CommandSender::new(host, &mut scratch).open(Open::slot(SETTINGS_SLOT)?.handover(handover))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_look_remembers_and_tells_nobody() {
        let mut shown = None;
        settle(&mut shown, Ok(7), || panic!("nothing changed"));
        assert_eq!(shown, Some(7));
    }

    #[test]
    fn a_change_is_queued_and_then_remembered() {
        let mut shown = Some(7);
        let mut told = 0;
        settle(&mut shown, Ok(8), || {
            told += 1;
            Ok(())
        });
        assert_eq!((shown, told), (Some(8), 1));
        settle(&mut shown, Ok(8), || panic!("nothing changed"));
    }

    #[test]
    fn a_refused_command_is_tried_again_at_the_next_tick() {
        let mut shown = Some(7);
        settle(&mut shown, Ok(8), || Err(Error::NoSpace));
        assert_eq!(shown, Some(7));
        let mut told = 0;
        settle(&mut shown, Ok(8), || {
            told += 1;
            Ok(())
        });
        assert_eq!((shown, told), (Some(8), 1));
    }

    #[test]
    fn a_cover_still_being_made_asks_for_a_new_draw_until_it_is_queued() {
        let mut wanted = false;
        retry(&mut wanted, || panic!("nothing was busy"));
        wanted = true;
        retry(&mut wanted, || Err(Error::NoSpace));
        assert!(wanted, "a refused command is tried again");
        let mut told = 0;
        retry(&mut wanted, || {
            told += 1;
            Ok(())
        });
        assert_eq!((wanted, told), (false, 1));
        retry(&mut wanted, || panic!("asked once"));
    }

    #[test]
    fn a_widget_that_cannot_tell_its_time_is_left_alone() {
        let mut shown = Some(7);
        settle(&mut shown, Err(Error::NoMemory), || panic!("no time"));
        assert_eq!(shown, Some(7));
    }

    #[test]
    fn one_failing_widget_does_not_stop_the_others() {
        let mut states = [Some(1), Some(1), Some(1)];
        let nows = [Ok(2), Err(Error::NoMemory), Ok(2)];
        let mut queued = 0;
        for (shown, now) in states.iter_mut().zip(nows) {
            settle(shown, now, || {
                queued += 1;
                Ok(())
            });
        }
        assert_eq!((states, queued), ([Some(2), Some(1), Some(2)], 2));
    }
}
