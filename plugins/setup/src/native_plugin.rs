//! The flat Setup plugin: the first-start check, the action that opens the
//! guide, and the guide's screen with its questions.
use crate::data::{self, Reader};
use crate::declarations::{EVENTS_TOKEN, GUIDE_TOKEN, OPEN_TOKEN};
use crate::model::{Guide, Kind, PageKind, Setting, Target, Turn, Val, turn};
use crate::view::{self, Layout};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{
    Handled, Point,
    native::{
        Error, Host, Plugin,
        callbacks::{DrawFrame, DrawPurpose, Input, InputGestureKind},
        commands::{CommandSender, PreviewSetting, RedrawMode},
        events::{AnswerOutcome, Event},
        fonts::{Font, FontRole},
        registrations,
        reply::Reply,
        settings::SettingValue,
        snapshot::capture_snapshot,
        ui::{Instance, Notify, Open, Question, RequestId, Ui},
    },
};
use paperpaw_ui_kit::{Messages, Screens, mem::buffer, paint, text::copy};

mod image {
    include!(concat!(env!("OUT_DIR"), "/setup_declarations.rs"));
}

const OPEN: &str = "setup.open";
const SLOT: &str = "core.setup";
const UI_BUFFER: usize = 4096;

/// What a question asked is waiting for.
#[derive(Debug)]
enum Asked {
    /// Whether to skip the whole setup (the second option does).
    Skip,
    /// The value of the option chosen for `key`.
    Choose { key: String, values: Vec<String> },
}

/// One guide on screen.
#[derive(Debug)]
struct View {
    guide: Guide,
    width: u16,
    height: u16,
    dpi: u16,
    /// Where things were when last drawn: what taps go to.
    layout: Layout,
}

/// Setup's interface-7 entry state.
#[derive(Debug, Default)]
pub struct NativeSetup {
    screens: Screens<View>,
    ui: Option<Ui>,
    asked: Option<(RequestId, Asked)>,
}

impl Plugin for NativeSetup {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::UI.iter().chain(image::EVENTS) {
            registrations::submit(host, batch)?;
        }
        Ok(Self::default())
    }

    fn event(
        &mut self,
        host: &Host<'_>,
        token: u32,
        event: Event<'_>,
        _: &mut Reply<'_>,
    ) -> Result<(), Error> {
        match (token, event) {
            (OPEN_TOKEN, Event::ActionRun { action: OPEN, .. }) => open_guide(host),
            (GUIDE_TOKEN, Event::ScreenShown { width, height, instance: Some(instance), .. }) => {
                self.shown(host, instance, width, height)
            }
            (GUIDE_TOKEN, Event::ScreenClosed { instance, .. }) => {
                self.screens.close(instance);
                Ok(())
            }
            (EVENTS_TOKEN, Event::Started { .. }) => first_start(host),
            (EVENTS_TOKEN, Event::Answered { answer, .. }) => self.answered(host, answer),
            (
                EVENTS_TOKEN,
                Event::SettingChanged { .. } | Event::WifiChanged { .. } | Event::RegistryChanged { .. },
            ) => self.reload_all(host),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        let view = self.screens.top(token).ok_or(Error::State)?;
        let scene = view::scene(host, &view.guide, view.width, view.height, view.dpi)?;
        view.layout = scene.layout;
        let font = Font::acquire_role(host, FontRole::Ui)?;
        let result = paint::replay(&mut frame, &font, &scene.paint);
        let released = font.release();
        result?;
        released
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        let Some(instance) = self.screens.top_instance(token) else {
            return Ok(Handled::No);
        };
        match input {
            Input::Action { action, .. } => self.action(host, instance, action),
            Input::Gesture { position, kind, .. } => self.gesture(host, instance, position, kind),
            _ => Ok(Handled::No),
        }
    }
}

/// Opens the guide in its slot (from the settings).
fn open_guide(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 128];
    CommandSender::new(host, &mut scratch).open(Open::slot(SLOT)?)
}

/// The first start: the guide, once. Not on a device used before it came, nor
/// where the done mark could not be written.
fn first_start(host: &Host<'_>) -> Result<(), Error> {
    if data::exists(host, data::DONE) {
        return Ok(());
    }
    if data::books_opened(host).unwrap_or(false) {
        // Used before the guide came: not a first start.
        if !data::mark_done(host) {
            return not_kept(host);
        }
        return Ok(());
    }
    if paperpaw_sdk::native::storage::write(host, paperpaw_sdk::native::storage::Root::Data, data::TRIAL, b"")
        .is_ok()
    {
        open_guide(host)?;
    }
    Ok(())
}

/// What tells that the done mark was not kept.
fn not_kept(host: &Host<'_>) -> Result<(), Error> {
    let mut messages = Messages::new()?;
    let text = messages.copy(host, "setup.not-kept")?;
    let mut scratch = [0; 512];
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}

/// Leaves the guide, done or skipped, and notes it.
fn leave(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).back()?;
    // What tells that it was not kept shows over the screen below.
    if !data::mark_done(host) {
        return not_kept(host);
    }
    Ok(())
}

fn redraw(host: &Host<'_>, mode: RedrawMode) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(mode)
}

/// Reads the step list and the page's setting into `guide`.
fn load(host: &Host<'_>, guide: &mut Guide, steps: bool) -> Result<(), Error> {
    let reader = Reader::open(host)?;
    let mut reader = reader;
    let result = (|| {
        if steps {
            guide.replace_steps(reader.steps()?);
        }
        guide.setting = match guide.step() {
            Some(step) if step.kind == PageKind::Setting => {
                let page = copy(&step.page)?;
                match reader.setting(&page) {
                    Ok(setting) => Some(setting),
                    // A step whose setting is gone shows its title alone.
                    Err(Error::NotFound) => None,
                    Err(error) => return Err(error),
                }
            }
            _ => None,
        };
        Ok(())
    })();
    reader.finish(result)
}

impl NativeSetup {
    fn ui(&mut self, host: &Host<'_>) -> Result<&Ui, Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        self.ui.as_ref().ok_or(Error::State)
    }

    /// A screen shows: its guide is made the first time, read anew after
    /// (back from a list or another screen: what was chosen shows).
    fn shown(&mut self, host: &Host<'_>, instance: Instance, width: u32, height: u32) -> Result<(), Error> {
        let width = u16::try_from(width).map_err(|_| Error::Invalid)?;
        let height = u16::try_from(height).map_err(|_| Error::Invalid)?;
        self.screens.shown(GUIDE_TOKEN, instance).or_else(|_| Ok::<(), Error>(()))?;
        if let Some(view) = self.screens.get(instance) {
            view.width = width;
            view.height = height;
            load(host, &mut view.guide, true)?;
            return redraw(host, RedrawMode::Partial);
        }
        let snapshot = capture_snapshot(host)?;
        let mut guide = Guide::new(Vec::new(), !snapshot.has_touch());
        load(host, &mut guide, true)?;
        self.screens.open(
            instance,
            View {
                guide,
                width,
                height,
                dpi: snapshot.display_dpi(),
                layout: Layout::default(),
            },
        )?;
        self.screens.shown(GUIDE_TOKEN, instance)
    }

    /// A step came or went, or a setting or the radio changed: every guide
    /// reads what it shows again.
    fn reload_all(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut any = false;
        for view in self.screens.all() {
            if view.guide.editing {
                continue;
            }
            load(host, &mut view.guide, true)?;
            any = true;
        }
        if any {
            redraw(host, RedrawMode::Fast)?;
        }
        Ok(())
    }

    fn answered(&mut self, host: &Host<'_>, answer: paperpaw_sdk::native::events::Answered) -> Result<(), Error> {
        let Some((request, _)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((_, asked)) = self.asked.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        let Ok(index) = usize::try_from(answer.choice()) else {
            return Ok(());
        };
        match asked {
            Asked::Skip if index == 1 => leave(host),
            Asked::Skip => Ok(()),
            Asked::Choose { key, values } => match values.get(index) {
                Some(value) => set(host, &key, SettingValue::Text(value)),
                None => Ok(()),
            },
        }
    }

    /// Asks whether to skip the setup: going on comes first.
    fn ask_skip(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let mut messages = Messages::new()?;
        let title = messages.copy(host, "setup.title")?;
        let text = messages.copy(host, "setup.skip-question")?;
        let stay = messages.copy(host, "setup.option.stay")?;
        let skip = messages.copy(host, "setup.option.skip")?;
        let choices = paperpaw_ui_kit::text::concat(&[&stay, "\n", &skip])?;
        let question = Question::choice(&title, &text, &choices)?;
        let (mut input, mut out) = (buffer(UI_BUFFER)?, buffer(UI_BUFFER)?);
        let request = self.ui(host)?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Skip));
        Ok(())
    }

    /// Asks which option of a choice of many: the list of their names.
    fn ask_choice(&mut self, host: &Host<'_>, setting: &Setting) -> Result<(), Error> {
        let mut names = String::new();
        let mut values = Vec::new();
        for (value, name) in &setting.choices {
            names
                .try_reserve(name.len() + 1)
                .map_err(|_| Error::NoMemory)?;
            if !names.is_empty() {
                names.push('\n');
            }
            names.push_str(name);
            values.try_reserve(1).map_err(|_| Error::NoMemory)?;
            values.push(copy(value)?);
        }
        let question = Question::choice(&setting.title, "", &names)?;
        let (mut input, mut out) = (buffer(UI_BUFFER)?, buffer(UI_BUFFER)?);
        let request = self.ui(host)?.ask(host, question, &mut input, &mut out)?;
        self.asked = Some((request, Asked::Choose { key: copy(&setting.key)?, values }));
        Ok(())
    }

    fn view(&mut self, instance: Instance) -> Result<&mut View, Error> {
        self.screens.get(instance).ok_or(Error::State)
    }

    /// Turns to `page` (or ends the guide past its last).
    fn turn(&mut self, host: &Host<'_>, instance: Instance, page: isize) -> Result<(), Error> {
        let view = self.view(instance)?;
        match turn(page, view.guide.steps.len()) {
            Turn::Stay => Ok(()),
            Turn::Finish => leave(host),
            Turn::To(page) => {
                if view.guide.editing {
                    end_edit(host, &mut view.guide, false)?;
                }
                view.guide.go(page);
                load(host, &mut view.guide, false)?;
                redraw(host, RedrawMode::Partial)
            }
        }
    }

    /// Chooses what `target` stands for.
    fn choose(&mut self, host: &Host<'_>, instance: Instance, target: Target) -> Result<(), Error> {
        let view = self.view(instance)?;
        let page = view.guide.page;
        match target {
            Target::Next => self.turn(host, instance, page as isize + 1),
            Target::Other if page == 0 => leave(host),
            Target::Other => self.turn(host, instance, page as isize - 1),
            Target::Card => self.card(host, instance),
        }
    }

    /// The card chosen: its screen, the next value, the list of values, or a
    /// number to change with the buttons.
    fn card(&mut self, host: &Host<'_>, instance: Instance) -> Result<(), Error> {
        let view = self.view(instance)?;
        let Some(step) = view.guide.step() else {
            return Ok(());
        };
        if step.kind == PageKind::Screen {
            let screen = copy(&step.page)?;
            let mut scratch = [0; 256];
            return CommandSender::new(host, &mut scratch).open_screen(&screen);
        }
        let Some(setting) = view.guide.setting.clone() else {
            return Ok(());
        };
        let shown = view.guide.shown;
        match setting.kind {
            Kind::Bool => {
                let on = matches!(setting.value, Some(Val::Bool(true)));
                set(host, &setting.key, SettingValue::Bool(!on))
            }
            Kind::Choice if setting.editor => Ok(()),
            Kind::Choice if setting.two().is_some() => self.pick_next(host, &setting),
            Kind::Choice => match self.ask_choice(host, &setting) {
                // Too long to list: the card steps through the options.
                Err(Error::NoSpace) => self.pick_next(host, &setting),
                other => other,
            },
            Kind::Int if shown => {
                let view = self.view(instance)?;
                view.guide.editing = true;
                redraw(host, RedrawMode::Fast)
            }
            _ => Ok(()),
        }
    }

    fn pick_next(&mut self, host: &Host<'_>, setting: &Setting) -> Result<(), Error> {
        match setting.next_choice() {
            Some(value) => set(host, &setting.key, SettingValue::Text(value)),
            None => Ok(()),
        }
    }

    /// Changes the number by `steps` (taps on − and +).
    fn change(&mut self, host: &Host<'_>, instance: Instance, steps: i64) -> Result<(), Error> {
        let view = self.view(instance)?;
        let Some(setting) = &view.guide.setting else {
            return Ok(());
        };
        match setting.moved(steps) {
            Some(value) => set(host, &setting.key.clone(), SettingValue::Int(value)),
            None => Ok(()),
        }
    }

    fn action(&mut self, host: &Host<'_>, instance: Instance, action: &str) -> Result<Handled, Error> {
        let view = self.view(instance)?;
        let page = view.guide.page as isize;
        // A number being changed: previewed with the buttons, kept by *Select*,
        // dropped by *Back*; the page buttons keep it and turn the page as ever.
        if view.guide.editing {
            match action {
                "core.next-page" | "core.prev-page" => end_edit(host, &mut view.guide, true)?,
                "core.confirm" => {
                    end_edit(host, &mut view.guide, true)?;
                    redraw(host, RedrawMode::Fast)?;
                    return Ok(Handled::Yes);
                }
                "core.back" => {
                    end_edit(host, &mut view.guide, false)?;
                    redraw(host, RedrawMode::Fast)?;
                    return Ok(Handled::Yes);
                }
                "core.left" | "core.up" | "core.previous" => {
                    edit_by(host, &mut view.guide, -1)?;
                    return Ok(Handled::Yes);
                }
                "core.right" | "core.down" | "core.next" => {
                    edit_by(host, &mut view.guide, 1)?;
                    return Ok(Handled::Yes);
                }
                _ => {}
            }
        }
        match action {
            "core.back" if page == 0 => self.ask_skip(host)?,
            "core.back" | "core.prev-page" => self.turn(host, instance, page - 1)?,
            "core.next-page" => self.turn(host, instance, page + 1)?,
            "core.up" | "core.left" | "core.previous" => self.focus(host, instance, -1)?,
            "core.down" | "core.right" | "core.next" => self.focus(host, instance, 1)?,
            "core.confirm" => {
                if view.guide.shown {
                    let target = view.guide.focus;
                    self.choose(host, instance, target)?;
                } else {
                    view.guide.shown = true;
                    redraw(host, RedrawMode::Fast)?;
                }
            }
            _ => return Ok(Handled::No),
        }
        Ok(Handled::Yes)
    }

    fn focus(&mut self, host: &Host<'_>, instance: Instance, delta: isize) -> Result<(), Error> {
        self.view(instance)?.guide.move_focus(delta);
        redraw(host, RedrawMode::Fast)
    }

    fn gesture(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        position: Point,
        kind: InputGestureKind,
    ) -> Result<Handled, Error> {
        if kind != InputGestureKind::Tap {
            return Ok(Handled::No);
        }
        let view = self.view(instance)?;
        view.guide.shown = false;
        if view.guide.editing {
            end_edit(host, &mut view.guide, true)?;
        }
        let layout = view.layout;
        if let Some((minus, plus)) = layout.stepper {
            if minus.contains(position) {
                self.change(host, instance, -1)?;
                return Ok(Handled::Yes);
            }
            if plus.contains(position) {
                self.change(host, instance, 1)?;
                return Ok(Handled::Yes);
            }
        }
        if let Some((left, right)) = layout.segments {
            for (index, rect) in [left, right].into_iter().enumerate() {
                if rect.contains(position) {
                    let setting = self.view(instance)?.guide.setting.clone();
                    if let Some(two) = setting.as_ref().and_then(Setting::two) {
                        set(host, &setting.as_ref().map_or("", |s| s.key.as_str()), SettingValue::Text(&two[index].0))?;
                    }
                    return Ok(Handled::Yes);
                }
            }
        }
        match layout.target_at(position) {
            Some(target) => {
                self.view(instance)?.guide.focus = target;
                self.choose(host, instance, target)?;
                Ok(Handled::Yes)
            }
            None => Ok(Handled::No),
        }
    }
}

/// Stages one setting change.
fn set(host: &Host<'_>, key: &str, value: SettingValue<'_>) -> Result<(), Error> {
    let mut scratch = [0; 1024];
    CommandSender::new(host, &mut scratch).set_setting(key, value)
}

/// Moves the number being changed and previews it.
fn edit_by(host: &Host<'_>, guide: &mut Guide, steps: i64) -> Result<(), Error> {
    let Some(setting) = guide.setting.as_mut() else {
        return Ok(());
    };
    let Some(next) = setting.moved(steps) else {
        return Ok(());
    };
    let (mut commands, mut values) = ([0; 512], [0; 32]);
    CommandSender::new(host, &mut commands).preview_settings(
        &[PreviewSetting::new(&setting.key, SettingValue::Int(next))],
        &mut values,
    )?;
    setting.value = Some(Val::Int(next));
    redraw(host, RedrawMode::Fast)
}

/// Ends changing the number: kept (set) or dropped (the preview ends, the
/// value is as it was).
fn end_edit(host: &Host<'_>, guide: &mut Guide, keep: bool) -> Result<(), Error> {
    guide.editing = false;
    let mut scratch = [0; 512];
    let mut sender = CommandSender::new(host, &mut scratch);
    sender.end_preview()?;
    if keep && let Some(setting) = &guide.setting && let Some(Val::Int(value)) = setting.value {
        let key = copy(&setting.key)?;
        drop(sender);
        return set(host, &key, SettingValue::Int(value));
    }
    drop(sender);
    load(host, guide, false)
}
