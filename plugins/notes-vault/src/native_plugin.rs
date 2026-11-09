//! The interface-7 notes vault: the vault's page, the lists of notes and the
//! tags as screens, the Tools entry that opens the page, and the index made in
//! the background by the work service while the page is open. The Markdown
//! format and the home widgets are later slices (`PORTING-7.md`).
use alloc::string::String;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::callbacks::{DrawFrame, DrawPurpose, Input};
use paperpaw_sdk::native::commands::{CommandSender, RedrawMode};
use paperpaw_sdk::native::events::{AnswerOutcome, Answered, Event, Incoming};
use paperpaw_sdk::native::settings::SettingValue;
use paperpaw_sdk::native::ui::{
    HandoverId, Instance, Notify, Open, Param, Question, RequestId, TextWrite, Ui,
};
use paperpaw_sdk::native::work::{Work, WorkSpec};
use paperpaw_sdk::native::{Error, Host, Plugin, registrations, reply::Reply};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::{Messages, Screens, uitext};

use crate::declarations::{HOME, LIST, OPEN, TAGS, WORK_DUE};
use crate::native_home::{self, Home, Next as HomeNext, Slice};
use crate::native_list::{self, List, MODE, Next as ListNext, WHAT};
use crate::native_tags::Tags;

mod image {
    include!(concat!(env!("OUT_DIR"), "/notes_vault_declarations.rs"));
}

/// The key of the work that makes the index.
const WORK_KEY: &str = "notes-vault.index";
/// The longest search the keyboard takes.
const SEARCH_LIMIT: usize = 256;
/// The longest folder the keyboard takes.
const FOLDER_LIMIT: usize = 512;
/// Room for a command besides its text.
const OVERHEAD: usize = 512;

/// What an open screen of this plugin is.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "kept unboxed: a box would allocate infallibly in a callback; the list grows fallibly"
)]
enum Slot {
    Home(Home),
    List(List),
    Tags(Tags),
}

/// What an open question is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    /// The words to search the titles for.
    Search,
    /// The vault's folder.
    Folder,
}

/// The interface-7 notes vault.
#[derive(Debug, Default)]
pub struct NativeNotesVault {
    ui: Option<Ui>,
    work: Option<Work>,
    screens: Screens<Slot>,
    asked: Option<(RequestId, Asked)>,
}

impl Plugin for NativeNotesVault {
    fn make(host: &Host<'_>) -> Result<Self, Error> {
        for batch in image::SETTINGS.iter().chain(image::UI) {
            registrations::submit(host, batch)?;
        }
        // `core.ui` is opened by the first callback: opening needs a live frame.
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
        self.ensure_ui(host)?;
        match event {
            Event::ActionRun { .. } if token == OPEN => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("notes-vault.screen")?)
            }
            Event::ScreenOpened {
                instance, handover, ..
            } => self.opened(host, token, instance, handover),
            Event::ScreenShown {
                instance: Some(instance),
                last_input,
                ..
            } => {
                self.screens.shown(token, instance)?;
                match self.screens.top(token) {
                    Some(Slot::Home(home)) => {
                        let work = home.shown(host, last_input)?;
                        if work {
                            self.declare_work(host)?;
                        }
                        Ok(())
                    }
                    Some(Slot::List(list)) => {
                        list.shown(last_input);
                        Ok(())
                    }
                    Some(Slot::Tags(tags)) => {
                        tags.shown(last_input);
                        Ok(())
                    }
                    None => Ok(()),
                }
            }
            Event::ScreenClosed { instance, .. } => {
                self.screens.close(instance);
                Ok(())
            }
            Event::WorkDue(due) if token == WORK_DUE && due.key() == WORK_KEY => {
                self.work_due(host)
            }
            Event::Answered { answer, .. } => self.answered(host, answer),
            _ => Ok(()),
        }
    }

    fn draw(&mut self, host: &Host<'_>, token: u32, mut frame: DrawFrame<'_>) -> Result<(), Error> {
        if !matches!(frame.purpose, DrawPurpose::Screen) {
            return Ok(());
        }
        match self.screens.top(token).ok_or(Error::State)? {
            Slot::Home(home) => home.draw(host, &mut frame),
            Slot::List(list) => list.draw(host, &mut frame),
            Slot::Tags(tags) => tags.draw(host, &mut frame),
        }
    }

    fn input(&mut self, host: &Host<'_>, token: u32, input: Input<'_>) -> Result<Handled, Error> {
        self.ensure_ui(host)?;
        match self.screens.top(token) {
            Some(Slot::Home(home)) => {
                let (handled, next) = home.input(host, &input)?;
                self.home_next(host, next)?;
                Ok(handled)
            }
            Some(Slot::List(list)) => {
                let (handled, next) = list.input(host, &input)?;
                match next {
                    ListNext::Folder(path) => self.open_list(host, "folder", &path)?,
                    ListNext::Note(path) => self.open_note(host, &path)?,
                    _ => {}
                }
                Ok(handled)
            }
            Some(Slot::Tags(tags)) => {
                let (handled, tag) = tags.input(host, &input)?;
                if let Some(tag) = tag {
                    self.open_list(host, "tag", &tag)?;
                }
                Ok(handled)
            }
            None => Ok(Handled::No),
        }
    }
}

impl NativeNotesVault {
    /// Opens `core.ui` in the first live callback; the handle serves later ones.
    fn ensure_ui(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.ui.is_none() {
            self.ui = Some(Ui::open(host)?);
        }
        Ok(())
    }

    fn ui(&self) -> Result<&Ui, Error> {
        self.ui.as_ref().ok_or(Error::State)
    }

    fn opened(
        &mut self,
        host: &Host<'_>,
        token: u32,
        instance: Instance,
        handover: Option<HandoverId>,
    ) -> Result<(), Error> {
        let slot = match token {
            HOME => {
                let home = Home::new(host)?;
                let work = home.running();
                let slot = Slot::Home(home);
                self.screens.open(instance, slot)?;
                if work {
                    self.declare_work(host)?;
                }
                return Ok(());
            }
            LIST => {
                let (mode, what) = match handover {
                    Some(handover) => {
                        let mut input = buffer(SERVICE_BUFFER)?;
                        let mut output = buffer(SERVICE_BUFFER)?;
                        native_list::params(
                            self.ui()?
                                .params(host, handover, &mut input, &mut output)?
                                .map(|param| param.map(|param| (param.key(), param.value()))),
                        )?
                    }
                    None => (None, String::new()),
                };
                Slot::List(List::new(host, mode.as_deref(), what)?)
            }
            TAGS => Slot::Tags(Tags::new(host)?),
            _ => return Err(Error::Invalid),
        };
        self.screens.open(instance, slot)
    }

    /// What the page's chosen row asks for.
    fn home_next(&mut self, host: &Host<'_>, next: HomeNext) -> Result<(), Error> {
        match next {
            HomeNext::Folders => self.open_list(host, "folder", ""),
            HomeNext::Recent => self.open_list(host, "recent", ""),
            HomeNext::Tags => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).open(Open::screen("notes-vault.tags")?)
            }
            HomeNext::Search => self.ask_search(host),
            HomeNext::Choose => self.ask_folder(host),
            HomeNext::Wait => say(host, "notes-vault.wait-index"),
            HomeNext::Work => {
                self.declare_work(host)?;
                redraw(host)
            }
            _ => Ok(()),
        }
    }

    /// Opens a list of notes: `mode` is what it shows, `what` its folder, tag
    /// or words.
    fn open_list(&self, host: &Host<'_>, mode: &str, what: &str) -> Result<(), Error> {
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let mode = Param::new(MODE, mode)?;
        let handover = if what.is_empty() {
            self.ui()?.handover(host, &[mode], &mut scratch, &mut out)?
        } else {
            let what = Param::new(WHAT, what)?;
            self.ui()?
                .handover(host, &[mode, what], &mut scratch, &mut out)?
        };
        CommandSender::new(host, &mut scratch)
            .open(Open::screen("notes-vault.list")?.handover(handover))
    }

    /// Opens the note at `path` in the reader the user bound to `core.reader`
    /// (a hand-over with its path, `document.md` section 1).
    fn open_note(&self, host: &Host<'_>, path: &str) -> Result<(), Error> {
        let mut scratch = buffer(SERVICE_BUFFER)?;
        let mut out = buffer(SERVICE_BUFFER)?;
        let handover =
            self.ui()?
                .handover(host, &[Param::new("path", path)?], &mut scratch, &mut out)?;
        CommandSender::new(host, &mut scratch).open(Open::slot("core.reader")?.handover(handover))
    }

    /// Declares the work that makes the next slice of the index; one at a time
    /// (the key replaces the one before).
    fn declare_work(&mut self, host: &Host<'_>) -> Result<(), Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        let work = self.work.as_ref().ok_or(Error::State)?;
        let mut input = [0u8; 256];
        work.declare(
            host,
            &WorkSpec::new(WORK_KEY, WORK_DUE)?,
            &mut input,
            &mut [],
        )
    }

    /// The index's work came due: one slice of it, and the page drawn anew to
    /// show how far it is.
    fn work_due(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(Slot::Home(home)) = self.screens.top(HOME) else {
            // The page is closed: the index is not made in the dark.
            return Ok(());
        };
        let slice = home.work(host);
        redraw(host)?;
        if slice? == Slice::More {
            self.declare_work(host)?;
        }
        Ok(())
    }

    /// Asks for the words to search the notes' titles for.
    fn ask_search(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "notes-vault.search")?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request = self
            .ui()?
            .ask(host, Question::text(&title, "")?, &mut input, &mut output)?;
        self.asked = Some((request, Asked::Search));
        Ok(())
    }

    /// Asks for the vault's folder, the one now chosen to begin with.
    fn ask_folder(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let title = Messages::new()?.copy(host, "notes-vault.choose")?;
        let current = native_home::vault(host)?;
        let mut input = buffer(SERVICE_BUFFER)?;
        let mut output = buffer(SERVICE_BUFFER)?;
        let request =
            self.ui()?
                .ask(host, Question::folder(&title, "")?, &mut input, &mut output)?;
        self.ui()?.text_write(
            host,
            request.text(),
            0,
            &current,
            TextWrite::Last,
            &mut input,
            &mut output,
        )?;
        self.asked = Some((request, Asked::Folder));
        Ok(())
    }

    fn answered(&mut self, host: &Host<'_>, answer: Answered) -> Result<(), Error> {
        let Some((request, asked)) = &self.asked else {
            return Ok(());
        };
        if *request != answer.request() || answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let (request, asked) = (*request, *asked);
        self.asked = None;
        if answer.outcome() != AnswerOutcome::Answered {
            return Ok(());
        }
        match asked {
            Asked::Search => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), SEARCH_LIMIT)?;
                let words = typed.trim();
                if words.is_empty() {
                    return Ok(());
                }
                self.open_list(host, "search", words)
            }
            Asked::Folder => {
                let typed = uitext::read_all(self.ui()?, host, request.text(), FOLDER_LIMIT)?;
                let folder = typed.trim().trim_matches('/');
                let mut scratch = buffer(OVERHEAD + folder.len())?;
                CommandSender::new(host, &mut scratch)
                    .set_setting(native_home::FOLDER, SettingValue::Text(folder))
            }
        }
    }
}

fn redraw(host: &Host<'_>) -> Result<(), Error> {
    let mut scratch = [0; 64];
    CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
}

/// Says the message of the catalog key `key`.
fn say(host: &Host<'_>, key: &str) -> Result<(), Error> {
    let text = Messages::new()?.copy(host, key)?;
    let mut scratch = buffer(OVERHEAD + text.len())?;
    CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
}
