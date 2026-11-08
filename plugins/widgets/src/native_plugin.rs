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
        events::{AnswerOutcome, Answered, Event},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        ui::{
            Instance, Open, Param, Question, RequestId, Ui, WidgetInputKind, WidgetTarget,
            stages::WidgetCall,
        },
    },
};
use paperpaw_ui_kit::paint;
use paperpaw_ui_kit::widget::Cx;

mod image {
    include!(concat!(env!("OUT_DIR"), "/widgets_declarations.rs"));
}

/// The callback token of the minute tick, as declared.
const TICK: u32 = crate::declarations::TICK;
/// The target of a widget whose tap opens its settings.
const SETTINGS_TARGET: u32 = 1;
/// The slot the settings fill.
const SETTINGS_SLOT: &str = "core.settings";
/// The most bytes of a stage reply (sizes and up to 32 targets).
const REPLY: usize = 1280;

#[derive(Debug)]
struct Item {
    instance: Instance,
    token: u32,
    /// The widget's id as the host names it, with its placement.
    widget: String,
    /// What it showed when it was last drawn or asked about, for the tick.
    shown: Option<u64>,
}

/// The widget pack.
#[derive(Debug, Default)]
pub struct NativeWidgets {
    items: Vec<Item>,
    /// The text asked for on the keyboard, and the setting it is for.
    asked: Option<(RequestId, String)>,
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
        });
        Ok(())
    }

    fn stage(&self, host: &Host<'_>, payload: &[u8], reply: &mut Reply<'_>) -> Result<(), Error> {
        let call = WidgetCall::decode(payload)?;
        let mut out = paperpaw_ui_kit::mem::buffer(REPLY)?;
        let written = match call {
            WidgetCall::Measure { instance, width } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
                let mut cx = Cx::open(host, &item.widget)?;
                let height = (behaviour.height)(&mut cx, u16::try_from(width).unwrap_or(u16::MAX))?;
                call.reply_size(u32::from(height), u32::from(height), &mut out)?
            }
            WidgetCall::Place { instance, .. } => {
                self.find(instance.get()).ok_or(Error::NotFound)?;
                call.reply_place(&mut out)?
            }
            WidgetCall::Targets { instance, rect } => {
                let item = self.find(instance.get()).ok_or(Error::NotFound)?;
                let tapped = behaviour(item.token).is_some_and(|b| b.tap != Tap::None);
                if tapped {
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
        let Some((request, setting)) = self.asked.take() else {
            return Ok(());
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
            settle(
                &mut item.shown,
                (behaviour.shows)(host, &item.widget),
                || commands.widget_changed(instance),
            );
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

impl Plugin for NativeWidgets {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::WIDGETS.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        reply: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::WidgetMade {
                widget,
                instance,
                now_ms,
                ..
            } => {
                if paperpaw_ui_kit::widget::settings::base(widget) == "widgets.quote" {
                    crate::native_quote::made(now_ms);
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
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(
        &mut self,
        host: &Host<'_>,
        _token: u32,
        mut frame: DrawFrame<'_>,
    ) -> Result<(), Error> {
        if frame.purpose != DrawPurpose::Widget {
            return Err(Error::Unsupported);
        }
        let item = self.find(frame.id).ok_or(Error::NotFound)?;
        let behaviour = behaviour(item.token).ok_or(Error::NotFound)?;
        let mut cx = Cx::open(host, &item.widget)?;
        let area = Rect::new(
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.x)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.y)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.width)),
            paperpaw_ui_kit::geometry::clamp_u16(i64::from(frame.rect.height)),
        );
        let mut out = Vec::new();
        (behaviour.draw)(&mut cx, &mut out, area)?;
        let bold = if out
            .iter()
            .any(|op| matches!(op, paperpaw_ui_kit::Paint::Bold { .. }))
        {
            Some(Font::acquire_role(host, FontRole::UiBold)?)
        } else {
            None
        };
        paint::replay_with(&mut frame, &cx.font, bold.as_ref(), &out)
    }

    fn input(&mut self, host: &Host<'_>, _: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Input::Widget {
            instance,
            target: SETTINGS_TARGET,
            kind: WidgetInputKind::Tap,
            ..
        } = input
        else {
            return Ok(Handled::No);
        };
        let item = self.find(instance.get()).ok_or(Error::NotFound)?;
        let base = paperpaw_ui_kit::widget::settings::base(&item.widget);
        match behaviour(item.token).map(|b| b.tap) {
            Some(Tap::Settings) => open_settings(host, base)?,
            Some(Tap::TurnQuote) => {
                crate::native_quote::turn(host, base)?;
                CommandSender::new(host, &mut [0; 64]).widget_changed(item.instance)?;
            }
            Some(Tap::Text { key, title }) => {
                // One request at a time: the keyboard of the first is still
                // open, and its answer is the one that counts.
                if self.asked.is_none() {
                    self.asked = Some(ask_text(host, base, key, title)?);
                }
            }
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }
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
