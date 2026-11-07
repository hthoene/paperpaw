//! The flat Setup plugin: the guide of the first start, one page a step the
//! plugins ask for, with the answer in a card and *Next* always right. It is
//! the screen of the slot `core.setup` and opens once, on the first start;
//! *Set up PaperPaw* in the settings opens it again.
use crate::native_guide::{Act, Fonts, Guide, HostMeasure, Texts, lay_out};
use crate::native_layout::Layout;
use crate::native_state;
use crate::native_steps::{Control, NewValue, Page, Reader, Setting};
use alloc::vec::Vec;
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, BookKind, Event},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::SettingValue,
        snapshot,
        ui::{Instance, Notify, Open, Question, QuestionKind, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens, Style,
    mem::{SERVICE_BUFFER, buffer},
    paint,
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/setup_declarations.rs"));
}

/// Callback tokens, as `declarations.rs` gives them.
const OPEN: u32 = 1;
const STARTED: u32 = 2;
const BOOK_OPENED: u32 = 3;
const GUIDE: u32 = 20;
/// The slot the guide fills.
const SLOT: &str = "core.setup";

/// The interface-7 Setup plugin.
#[derive(Debug, Default)]
pub struct NativeSetup {
    ui: Option<Ui>,
    screens: Screens<Guide>,
    /// The question whether to skip, while it is open.
    asked: Option<RequestId>,
}

impl Plugin for NativeSetup {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SCREENS
            .iter()
            .chain(image::SLOTS)
            .chain(image::NAVIGATION)
            .chain(image::EVENTS)
        {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback that needs it: opening
        // needs a live frame.
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match event {
            Event::ActionRun { .. } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::slot(SLOT)?)
            }
            Event::Started { .. } if token == STARTED => started(host),
            // The first book opened, by the user (reason 0), is noted once.
            Event::Book {
                kind: BookKind::Opened,
                reason: 0,
                ..
            } if token == BOOK_OPENED => native_state::note_book_opened(host),
            Event::ScreenOpened {
                screen: "setup.guide",
                instance,
                ..
            } => self.opened(host, instance),
            Event::ScreenShown {
                screen: "setup.guide",
                instance: Some(instance),
                ..
            } => self.screens.shown(GUIDE, instance),
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let guide = self.screens.top(token).ok_or(Error::State)?;
        let ui = Font::acquire_role(host, FontRole::Ui)?;
        let reading = Font::acquire_role(host, FontRole::Reading).ok();
        let result = (|| {
            let display = snapshot::capture_snapshot(host)?;
            let style = Style::read(host, &display, &ui)?;
            let measure = HostMeasure {
                ui: &ui,
                reading: reading.as_ref(),
                sample: &guide.texts.sample,
            };
            let layout = lay_out(guide, &style, &measure);
            let mut ops = Vec::new();
            let fonts = Fonts {
                ui: &ui,
                reading: reading.as_ref(),
            };
            guide.prepare(&mut ops, &fonts, &style, &layout)?;
            paint::replay(&mut frame, &ui, ops.iter())
        })();
        let reading = reading.map(Font::release).transpose();
        let ui = ui.release();
        result?;
        reading?;
        ui
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(instance) = self.screens.top_instance(token) else {
            return Ok(Handled::No);
        };
        let Some(guide) = self.screens.get(instance) else {
            return Ok(Handled::No);
        };
        let layout = layout_of(host, guide)?;
        let act = guide.decide(&layout, &input);
        let handled = if act == Act::Ignore {
            Handled::No
        } else {
            Handled::Yes
        };
        self.carry_out(host, instance, act)?;
        Ok(handled)
    }
}

/// The page's layout: the fonts and the style, read afresh (the callback owns
/// them).
fn layout_of(host: &Host<'_>, guide: &Guide) -> Result<Layout, Error> {
    let ui = Font::acquire_role(host, FontRole::Ui)?;
    let reading = Font::acquire_role(host, FontRole::Reading).ok();
    let result = (|| {
        let display = snapshot::capture_snapshot(host)?;
        let style = Style::read(host, &display, &ui)?;
        let measure = HostMeasure {
            ui: &ui,
            reading: reading.as_ref(),
            sample: &guide.texts.sample,
        };
        Ok(lay_out(guide, &style, &measure))
    })();
    let reading = reading.map(Font::release).transpose();
    let ui = ui.release();
    let layout = result?;
    reading?;
    ui?;
    Ok(layout)
}

/// The first start: the guide, once. Not on a device that was used before the
/// guide came, nor where the storage cannot be written.
fn started(host: &Host<'_>) -> Result<(), Error> {
    if native_state::is_done(host)? {
        return Ok(());
    }
    if native_state::has_opened_book(host)? {
        // Used before: not a first start.
        if !native_state::mark_done(host) {
            return notify(host, "setup.not-kept");
        }
    } else if native_state::try_trial(host) {
        let mut scratch = [0; 128];
        CommandSender::new(host, &mut scratch).open(Open::slot(SLOT)?)?;
    }
    Ok(())
}

fn notify(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(SERVICE_BUFFER)?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

fn texts(host: &Host<'_>) -> Result<Texts, Error> {
    let mut m = Messages::new()?;
    Ok(Texts {
        next: m.copy(host, "setup.next")?,
        done: m.copy(host, "setup.done")?,
        skip: m.copy(host, "setup.skip")?,
        skip_short: m.copy(host, "setup.skip-short")?,
        back: m.copy(host, "setup.back")?,
        open: m.copy(host, "setup.open-page")?,
        on: m.copy(host, "setup.on")?,
        off: m.copy(host, "setup.off")?,
        sample: m.copy(host, "setup.sample")?,
        end_title: m.copy(host, "setup.end.title")?,
        end_help: m.copy(host, "setup.end.help")?,
    })
}

impl NativeSetup {
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn opened(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let reader = Reader::open(host)?;
        let steps = {
            let mut reader = reader;
            let found = reader.steps();
            reader.finish(found)?
        };
        let snap = snapshot::capture_snapshot(host)?;
        let display = (
            snap.display_width(),
            snap.display_height(),
            snap.display_dpi(),
        );
        let mut guide = Guide::new(steps, texts(host)?, display, snap.has_touch());
        let loaded = read_page(host, &guide, 0)?;
        guide.commit(0, loaded.gone, loaded.setting);
        self.screens.open(instance, guide)
    }

    fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(mode)
    }

    /// Carries out what an input meant. The host's command comes first and
    /// the guide's state follows it, so a refused command leaves the state as
    /// the screen shows it.
    fn carry_out(&mut self, host: &Host<'_>, instance: Instance, act: Act) -> Result<(), Error> {
        let Some(guide) = self.screens.get(instance) else {
            return Ok(());
        };
        match act {
            Act::Ignore | Act::Keep => Ok(()),
            Act::Focus(target) => {
                Self::redraw(host, RedrawMode::Partial)?;
                guide.focus = target;
                guide.shown = true;
                Ok(())
            }
            Act::Turn(page) if page > guide.steps.len() => finish(host),
            Act::Turn(page) => {
                let loaded = read_page(host, guide, page)?;
                Self::redraw(host, RedrawMode::Partial)?;
                guide.commit(page, loaded.gone, loaded.setting);
                Ok(())
            }
            Act::AskSkip => self.ask_skip(host),
            Act::Set(value) => {
                let Some(setting) = guide.setting.as_mut() else {
                    return Ok(());
                };
                let mut scratch = [0; 512];
                let mut commands = CommandSender::new(host, &mut scratch);
                match (&setting.control, value) {
                    (_, NewValue::Bool(on)) => {
                        commands.set_setting(&setting.key, SettingValue::Bool(on))?;
                    }
                    (_, NewValue::Int(number)) => {
                        commands.set_setting(&setting.key, SettingValue::Int(number))?;
                    }
                    (Control::Choice { options, .. }, NewValue::Text(at)) => {
                        let Some(option) = options.get(at) else {
                            return Ok(());
                        };
                        commands.set_setting(&setting.key, SettingValue::Text(&option.value))?;
                    }
                    _ => return Ok(()),
                }
                Self::redraw(host, RedrawMode::Fast)?;
                setting.apply(&value);
                Ok(())
            }
            Act::OpenScreen => {
                let Some(screen) = guide.screen_step() else {
                    return Ok(());
                };
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch).open(Open::screen(screen)?)
            }
            Act::OpenEditor => {
                let Some((slot, id)) = guide.editor() else {
                    return Ok(());
                };
                let mut scratch = [0; 256];
                let open = if slot {
                    Open::slot(id)?
                } else {
                    Open::screen(id)?
                };
                CommandSender::new(host, &mut scratch).open(open)
            }
        }
    }

    /// Asks whether to skip the setup: going on comes first.
    fn ask_skip(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let mut m = Messages::new()?;
        let title = m.copy(host, "setup.title")?;
        let question = m.copy(host, "setup.skip-question")?;
        let mut choices = m.copy(host, "setup.option.stay")?;
        choices.try_reserve(1).map_err(|_| Error::NoMemory)?;
        choices.push('\n');
        let skip = m.copy(host, "setup.option.skip")?;
        choices
            .try_reserve(skip.len())
            .map_err(|_| Error::NoMemory)?;
        choices.push_str(&skip);
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let request = ui.ask(
            host,
            Question::choice(&title, &question, &choices)?,
            &mut input,
            &mut output,
        )?;
        self.asked = Some(request);
        Ok(())
    }

    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some(request) = self.asked else {
            return Ok(());
        };
        if request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        self.asked = None;
        if answer.outcome() == AnswerOutcome::Answered
            && answer.kind() == QuestionKind::Choice
            && answer.choice() == 1
        {
            return finish(host);
        }
        Ok(())
    }
}

/// Leaves the guide, the setup done or skipped; what tells that it was not
/// kept shows over the screen below.
fn finish(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back()?;
    if native_state::mark_done(host) {
        Ok(())
    } else {
        notify(host, "setup.not-kept")
    }
}

/// A page read before it is shown: the setting it asks for, and how many
/// steps from the page on have no setting any more and are left out.
struct Loaded {
    gone: usize,
    setting: Option<Setting>,
}

/// Reads the setting of page `page`; a step whose setting is gone is left
/// out (counted, not removed: nothing changes before the whole read is done).
fn read_page(host: &Host<'_>, guide: &Guide, page: usize) -> Result<Loaded, Error> {
    let mut gone = 0;
    loop {
        let Some(step) = guide.steps.get(page + gone) else {
            return Ok(Loaded {
                gone,
                setting: None,
            });
        };
        let Page::Setting(key) = &step.page else {
            return Ok(Loaded {
                gone,
                setting: None,
            });
        };
        let mut reader = Reader::open(host)?;
        let found = reader.setting(key);
        match reader.finish(found) {
            Ok(setting) => {
                return Ok(Loaded {
                    gone,
                    setting: Some(setting),
                });
            }
            Err(Error::NotFound) => gone += 1,
            Err(error) => return Err(error),
        }
    }
}
