//! The flat Setup plugin: the guide of the first start, one page a step the
//! plugins ask for, with the answer in a card and *Next* always right. It is
//! the screen of the slot `core.setup` and opens once, on the first start;
//! *Set up PaperPaw* in the settings opens it again.
use crate::native_guide::{Act, Fonts, Guide, HostMeasure, Texts, lay_out};
use crate::native_layout::Layout;
use crate::native_state;
use crate::native_steps::{Control, NewValue, Page, Reader, Setting};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input},
        commands::{CommandSender, RedrawMode},
        events::{AnswerOutcome, Answered, BookKind, Event, Incoming},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::SettingValue,
        snapshot,
        ui::{ButtonHint, Instance, Notify, Open, Param, Question, QuestionKind, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{
    Messages, Screens, Style, font_sizes,
    mem::{SERVICE_BUFFER, buffer},
    paint,
    text::{concat, copy},
};

mod image {
    include!(concat!(env!("OUT_DIR"), "/setup_declarations.rs"));
}

/// Callback tokens, as `declarations.rs` gives them.
const OPEN: u32 = 1;
const STARTED: u32 = 2;
const BOOK_OPENED: u32 = 3;
const SETTING_CHANGED: u32 = 4;
const GUIDE: u32 = 20;
/// The setting whose change the guide's words follow.
const LANGUAGE: &str = "core.language";
/// The slot the guide fills.
const SLOT: &str = "core.setup";
/// The slot of the screen that lists a setting's options to choose from.
const CHOOSE_VALUE: &str = "core.choose-value";

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
        incoming: Incoming<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        let event = incoming.decode()?;
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
            } if token == BOOK_OPENED => {
                native_state::note_book_opened(host);
                Ok(())
            }
            // The language (or several settings at once, the empty key): the
            // guide's words and the page are read again in it.
            Event::SettingChanged { key, .. }
                if token == SETTING_CHANGED && (key == LANGUAGE || key.is_empty()) =>
            {
                self.language_changed(host)
            }
            Event::ScreenOpened {
                screen: "setup.guide",
                instance,
                ..
            } => self.opened(host, instance),
            Event::ScreenShown {
                screen: "setup.guide",
                instance: Some(instance),
                ..
            } => self.shown(host, instance),
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
            let (mut ops, mut book) = (Vec::new(), Vec::new());
            let fonts = Fonts {
                ui: &ui,
                reading: reading.as_ref(),
            };
            guide.prepare(&mut ops, &mut book, &fonts, &style, &layout)?;
            paint::replay(&mut frame, &ui, ops.iter())?;
            match &reading {
                Some(reading) => paint::replay(&mut frame, reading, book.iter()),
                None => Ok(()),
            }
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
        keep: m.copy(host, "core.hint.keep")?,
        cancel: m.copy(host, "core.hint.cancel")?,
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
        guide.commit(0, loaded.gone, (loaded.setting, loaded.help));
        key_bar(host, &guide);
        self.screens.open(instance, guide)
    }

    /// The language changed: each open guide takes its words, its steps'
    /// titles and its page anew and is drawn again, its key bar too (#459,
    /// #472: until then the title and *Skip* stayed in the language the guide
    /// opened in).
    fn language_changed(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut any = false;
        for guide in self.screens.all() {
            guide.texts = texts(host)?;
            let mut reader = Reader::open(host)?;
            let found = reader.steps();
            guide.retitle(reader.finish(found)?);
            let loaded = read_page(host, guide, guide.page)?;
            guide.commit(guide.page, loaded.gone, (loaded.setting, loaded.help));
            key_bar(host, guide);
            any = true;
        }
        if any {
            Self::redraw(host, RedrawMode::Partial)?;
        }
        Ok(())
    }

    /// The guide shows again: back from a screen opened from its page (the
    /// list of a choice, an editor), the page's setting is read again, as
    /// it was chosen there.
    fn shown(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        self.screens.shown(GUIDE, instance)?;
        let Some(guide) = self.screens.get(instance) else {
            return Ok(());
        };
        if !guide.away {
            return Ok(());
        }
        let loaded = read_page(host, guide, guide.page)?;
        Self::redraw(host, RedrawMode::Partial)?;
        guide.commit(guide.page, loaded.gone, (loaded.setting, loaded.help));
        guide.away = false;
        key_bar(host, guide);
        Ok(())
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
            Act::Turn(page) if page > guide.steps.len() => {
                keep_edited(host, guide)?;
                finish(host)
            }
            Act::Turn(page) => {
                keep_edited(host, guide)?;
                let loaded = read_page(host, guide, page)?;
                Self::redraw(host, RedrawMode::Partial)?;
                guide.commit(page, loaded.gone, (loaded.setting, loaded.help));
                key_bar(host, guide);
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
            Act::Step(delta) => step(host, guide, delta),
            Act::Edit => start_edit(host, guide),
            Act::EndEdit(keep) => end_edit(host, guide, keep),
            Act::OpenScreen => {
                let Some(screen) = guide.screen_step() else {
                    return Ok(());
                };
                let mut scratch = [0; 256];
                CommandSender::new(host, &mut scratch).open(Open::screen(screen)?)?;
                guide.away = true;
                Ok(())
            }
            Act::OpenEditor => {
                let (Some((slot, id)), Some(setting)) = (guide.editor(), guide.setting.as_ref())
                else {
                    return Ok(());
                };
                let (id, key) = (copy(id)?, copy(&setting.key)?);
                let open = if slot {
                    Open::slot(&id)?
                } else {
                    Open::screen(&id)?
                };
                self.open_for(host, instance, open, &key)
            }
            Act::OpenList => {
                let Some(setting) = guide.setting.as_ref() else {
                    return Ok(());
                };
                let key = copy(&setting.key)?;
                self.open_for(host, instance, Open::slot(CHOOSE_VALUE)?, &key)
            }
        }
    }

    /// Opens `open` for setting `key` (handed over as `id`, as the settings
    /// pages do): its editor, or the list of its options.
    fn open_for(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        open: Open<'_>,
        key: &str,
    ) -> Result<(), Error> {
        self.ensure_ui(host)?;
        let params = [Param::new("id", key)?];
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        let handover = ui.handover(host, &params, &mut input, &mut output)?;
        let mut scratch = buffer(1024)?;
        CommandSender::new(host, &mut scratch).open(open.handover(handover))?;
        if let Some(guide) = self.screens.get(instance) {
            guide.away = true;
        }
        Ok(())
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

/// Moves the number shown `delta` steps: the size of book text to the next
/// size the reading font draws differently ([`font_sizes`]). While it is
/// changed with the buttons only the page shows it; otherwise it is set.
fn step(host: &Host<'_>, guide: &mut Guide, delta: i64) -> Result<(), Error> {
    let Some(setting) = guide.setting.as_mut() else {
        return Ok(());
    };
    let Control::Number {
        value, min, max, ..
    } = setting.control
    else {
        return Ok(());
    };
    let Some(NewValue::Int(to)) = setting.next(delta) else {
        return Ok(());
    };
    let next = font_sizes::step(host, &setting.key, (value, to), (min, max))?;
    if next == value {
        return Ok(());
    }
    if guide.editing.is_none() {
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch)
            .set_setting(&setting.key, SettingValue::Int(next))?;
    }
    NativeSetup::redraw(host, RedrawMode::Fast)?;
    setting.apply(&NewValue::Int(next));
    Ok(())
}

/// Starts changing the number with the buttons.
fn start_edit(host: &Host<'_>, guide: &mut Guide) -> Result<(), Error> {
    let Some(Setting {
        control: Control::Number { value, .. },
        ..
    }) = guide.setting
    else {
        return Ok(());
    };
    NativeSetup::redraw(host, RedrawMode::Fast)?;
    guide.editing = Some(value);
    key_bar(host, guide);
    Ok(())
}

/// Ends changing the number with the buttons: keeps the value shown, or goes
/// back to the one before.
fn end_edit(host: &Host<'_>, guide: &mut Guide, keep: bool) -> Result<(), Error> {
    if keep {
        keep_edited(host, guide)?;
    } else if let (Some(before), Some(setting)) = (guide.editing, guide.setting.as_mut()) {
        setting.apply(&NewValue::Int(before));
    }
    NativeSetup::redraw(host, RedrawMode::Partial)?;
    guide.editing = None;
    key_bar(host, guide);
    Ok(())
}

/// Ends changing the number with the buttons, keeping the value shown: set
/// where it changed.
fn keep_edited(host: &Host<'_>, guide: &mut Guide) -> Result<(), Error> {
    let (Some(before), Some(setting)) = (guide.editing, guide.setting.as_ref()) else {
        return Ok(());
    };
    if let Control::Number { value, .. } = setting.control
        && value != before
    {
        let mut scratch = [0; 512];
        CommandSender::new(host, &mut scratch)
            .set_setting(&setting.key, SettingValue::Int(value))?;
    }
    guide.editing = None;
    Ok(())
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
    /// What a screen step says about itself.
    help: String,
}

/// What the plugin of a screen step says about it: its catalog's `<id>.help`
/// (the registry's step record has no text for it); none where it has none.
fn screen_help(host: &Host<'_>, id: &str) -> String {
    let Ok(key) = concat(&[id, ".help"]) else {
        return String::new();
    };
    Messages::new()
        .and_then(|mut m| m.copy(host, &key))
        .unwrap_or_default()
}

/// The key bar of the page: on the first, *Back* asks to skip and says so
/// (*Skip*, as in Beta 3); while a number is changed with the buttons they say
/// *Cancel*, *Keep*, − and +; anywhere else the buttons have their usual
/// names. A hint the host refuses leaves the bar as it was.
fn key_bar(host: &Host<'_>, guide: &Guide) {
    let t = &guide.texts;
    let words = if guide.editing.is_some() {
        [
            Some(t.cancel.as_str()),
            Some(t.keep.as_str()),
            Some("\u{2212}"),
            Some("+"),
        ]
    } else if guide.page == 0 && !t.skip_short.is_empty() {
        [Some(t.skip_short.as_str()), None, None, None]
    } else {
        [None; 4]
    };
    let actions = ["core.back", "core.confirm", "core.left", "core.right"];
    for (action, word) in actions.into_iter().zip(words) {
        let hint = match word {
            Some(word) => ButtonHint::words(action, word),
            None => ButtonHint::usual(action),
        };
        if let Ok(hint) = hint {
            match CommandSender::new(host, &mut [0; 256]).set_hint(hint) {
                Ok(()) | Err(_) => {}
            }
        }
    }
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
                help: String::new(),
            });
        };
        let Page::Setting(key) = &step.page else {
            return Ok(Loaded {
                gone,
                setting: None,
                help: screen_help(host, &step.id),
            });
        };
        let mut reader = Reader::open(host)?;
        let found = reader.setting(key);
        match reader.finish(found) {
            Ok(setting) => {
                return Ok(Loaded {
                    gone,
                    setting: Some(setting),
                    help: String::new(),
                });
            }
            Err(Error::NotFound) => gone += 1,
            Err(error) => return Err(error),
        }
    }
}
