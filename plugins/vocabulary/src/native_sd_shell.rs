//! The card files' screens in the shell: the menu (`vocabulary.sd-menu`) and the
//! screen that updates from the files or saves a deck (`vocabulary.sd-card`).
//! The decks, the ledger and the card's folder are one [`Storage`] for a
//! callback ([`with_both`]); the update and the save go a bounded step at a
//! time, each step a `core.work` callback that asks for the next while there
//! is one (the host calls back between the reader's keys, so the screen stays
//! alive).
use alloc::string::String;

use paperpaw_sdk::Handled;
use paperpaw_sdk::native::{
    Error, Host,
    callbacks::{DrawFrame, Input},
    commands::{CommandSender, RedrawMode},
    settings::{SettingValue, read_setting},
    ui::{Instance, Open, Param},
    work::{Work, WorkSpec},
};
use paperpaw_ui_kit::mem::{SERVICE_BUFFER, buffer};
use paperpaw_ui_kit::{Footer, ItemList, ItemStep, text};

use crate::declarations::{SD_CARD_TOKEN, SD_MENU_TOKEN, WORK_TOKEN};
use crate::native_card::with_both;
use crate::native_plugin::State;
use crate::native_sd::{self, CARD, MenuView, Run, Shown, Target};
use crate::native_words::{HostWords, Words};
use crate::{cards, sync};

/// The setting that names the folder of the card files, and the one that lets
/// the flash cards update from them when they open.
const FOLDER_SETTING: &str = "vocabulary.sd-folder";
const AUTO_SETTING: &str = "vocabulary.sd-auto";
/// The key of the work that does the next step.
const WORK_KEY: &str = "vocabulary.sd";
/// The screen title.
const TITLE: &str = "vocabulary.sd.title";

/// The folder the setting names (the default where it is empty or unusable).
///
/// # Errors
/// [`Error::NoMemory`] and what the host says.
pub(crate) fn folder(host: &Host<'_>) -> Result<String, Error> {
    let mut scratch = [0u8; 320];
    let setting = match read_setting(host, FOLDER_SETTING, &mut scratch)? {
        SettingValue::Text(text) => text,
        _ => "",
    };
    native_sd::folder_of(setting)
}

/// Whether the flash cards update from the files when they open.
///
/// # Errors
/// What the host says.
pub(crate) fn auto(host: &Host<'_>) -> Result<bool, Error> {
    let mut scratch = [0u8; 64];
    Ok(matches!(
        read_setting(host, AUTO_SETTING, &mut scratch)?,
        SettingValue::Bool(true) | SettingValue::Absent
    ))
}

/// Declares the work that does the next step.
fn schedule(host: &Host<'_>) -> Result<(), Error> {
    let work = Work::open(host)?;
    let (mut input, mut reply) = ([0u8; 256], [0u8; 64]);
    let declared = WorkSpec::new(WORK_KEY, WORK_TOKEN)
        .and_then(|spec| work.declare(host, &spec, &mut input, &mut reply));
    let released = work.release(host);
    declared.and(released)
}

/// Ends a run whose step could not be done (the card is gone, the heap refuses
/// the service's buffers) with a line that says why, so that it never waits for
/// a step that does not come.
fn end_run(host: &Host<'_>, run: &mut Run, error: Error) {
    let key = if error == Error::NoMemory {
        "vocabulary.sd.nomemory"
    } else {
        "vocabulary.sd.nocard"
    };
    let note = HostWords::new(host)
        .and_then(|mut words| words.copy(key))
        .ok();
    run.abort(note);
    // Best effort: the screen is drawn again at its next input otherwise.
    let _ = CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial);
}

/// Opens `screen`, the hand-over carrying `params`.
fn open_with(
    host: &Host<'_>,
    ui: &paperpaw_sdk::native::ui::Ui,
    screen: &str,
    params: &[(&str, &str)],
) -> Result<(), Error> {
    let mut input = buffer(SERVICE_BUFFER)?;
    let mut output = buffer(SERVICE_BUFFER)?;
    let mut open = Open::screen(screen)?;
    if let Some((key, value)) = params.first() {
        let param = Param::new(key, value)?;
        open = open.handover(ui.handover(host, &[param], &mut input, &mut output)?);
    }
    CommandSender::new(host, &mut input).open(open)
}

/// What the decks, the ledger and the files hold, as the menu shows it.
fn menu_view(host: &Host<'_>, old: Option<&mut MenuView>) -> Result<Option<MenuView>, Error> {
    let folder = folder(host)?;
    let (decks, ledger, pending) = with_both(host, |storage| {
        let decks = cards::decks(storage).map_err(|_| Error::NoMemory)?;
        let ledger = sync::sources(storage).map_err(|_| Error::NoMemory)?;
        let pending = sync::pending(storage, &folder).unwrap_or_default();
        Ok((decks, ledger, pending))
    })?;
    let shown = Shown {
        folder: &folder,
        pending,
        decks: &decks,
        ledger: &ledger,
    };
    let mut words = HostWords::new(host)?;
    match old {
        Some(view) => {
            view.refresh(&mut words, shown)?;
            Ok(None)
        }
        None => MenuView::new(&mut words, shown).map(Some),
    }
}

impl State {
    /// The menu opened: the files and the decks as they are.
    pub(crate) fn opened_sd_menu(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let view = menu_view(host, None)?.ok_or(Error::State)?;
        self.sd_menus.open(instance, view)
    }

    /// The menu shown again (an update changed the decks): read again.
    pub(crate) fn shown_sd_menu(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
    ) -> Result<(), Error> {
        self.sd_menus.shown(SD_MENU_TOKEN, instance)?;
        if let Some(view) = self.sd_menus.get(instance) {
            menu_view(host, Some(view))?;
        }
        CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)
    }

    /// The update (or the save) opened: the first step is asked for.
    pub(crate) fn opened_sd_card(
        &mut self,
        host: &Host<'_>,
        instance: Instance,
        handover: Option<paperpaw_sdk::native::ui::HandoverId>,
    ) -> Result<(), Error> {
        self.ensure(host)?;
        let (mode, save) = match handover {
            Some(handover) => {
                let (mut input, mut out) = (buffer(4096)?, buffer(4096)?);
                let params = self
                    .ui
                    .as_ref()
                    .ok_or(Error::State)?
                    .params(host, handover, &mut input, &mut out)?;
                native_sd::named(params.map(|param| param.map(|p| (p.key(), p.value()))))?
            }
            None => (sync::Mode::Manual, None),
        };
        let run = Run::new(&folder(host)?, mode, save)?;
        self.sd_runs.open(instance, run)?;
        // On top from the start: the work step and the first draw find it.
        self.sd_runs.shown(SD_CARD_TOKEN, instance)?;
        if let Err(error) = schedule(host)
            && let Some(run) = self.sd_runs.get(instance)
        {
            end_run(host, run, error);
        }
        Ok(())
    }

    /// One step of the update or the save, and the next asked for.
    pub(crate) fn sd_work(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(instance) = self.sd_runs.top_instance(SD_CARD_TOKEN) else {
            return Ok(());
        };
        let Some(run) = self.sd_runs.get(instance) else {
            return Ok(());
        };
        if !run.working() {
            return Ok(());
        }
        let stepped = HostWords::new(host)
            .and_then(|mut words| with_both(host, |storage| Ok(run.step(storage, &mut words))));
        match stepped {
            Ok(next) => {
                if next.redraw {
                    CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)?;
                }
                if next.more
                    && let Err(error) = schedule(host)
                {
                    end_run(host, run, error);
                }
            }
            // The card is gone, or the heap refuses: the run ends with a line.
            Err(error) => end_run(host, run, error),
        }
        Ok(())
    }

    /// A screen of the card files closed: a save left off leaves no part of a
    /// file behind.
    pub(crate) fn closed_sd(&mut self, host: &Host<'_>, instance: Instance) {
        self.sd_menus.close(instance);
        if let Some(run) = self.sd_runs.get(instance) {
            // Best effort: a part left is removed by the next save.
            let _ = with_both(host, |storage| {
                run.cancel(storage);
                Ok(())
            });
        }
        self.sd_runs.close(instance);
    }

    /// Draws the menu or the update screen.
    pub(crate) fn draw_sd(
        &mut self,
        host: &Host<'_>,
        token: u32,
        frame: &mut DrawFrame<'_>,
    ) -> Result<(), Error> {
        if token == SD_MENU_TOKEN {
            let view = self.sd_menus.top(token).ok_or(Error::State)?;
            return view
                .list()
                .draw_with(host, frame, TITLE, true, Footer::None);
        }
        let run = self.sd_runs.top(token).ok_or(Error::State)?;
        run.show(&mut HostWords::new(host)?)?;
        run.list().draw_with(host, frame, TITLE, true, Footer::None)
    }

    /// Input of the menu or the update screen.
    pub(crate) fn input_sd(
        &mut self,
        host: &Host<'_>,
        token: u32,
        input: &Input<'_>,
    ) -> Result<Handled, Error> {
        if token == SD_MENU_TOKEN {
            return self.input_menu(host, input);
        }
        let Some(run) = self.sd_runs.top(token) else {
            return Ok(Handled::No);
        };
        if run.working() {
            // Only the back arrow does anything while it works (the host's).
            return Ok(Handled::Yes);
        }
        let asking = run.asking();
        let step = run.list().input(host, input)?;
        if asking && let ItemStep::Chosen(index) = step {
            // Row 0 is the question; 1 adds to the deck there, 2 makes a new one.
            if index == 1 || index == 2 {
                let run = self.sd_runs.top(token).ok_or(Error::State)?;
                let answered = with_both(host, |storage| {
                    run.answer(storage, index == 1);
                    Ok(())
                })
                .and_then(|()| schedule(host));
                match answered {
                    Ok(()) => {
                        CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)?;
                    }
                    Err(error) => end_run(host, run, error),
                }
            }
        }
        Ok(ItemList::handled(step))
    }

    fn input_menu(&mut self, host: &Host<'_>, input: &Input<'_>) -> Result<Handled, Error> {
        let view = self.sd_menus.top(SD_MENU_TOKEN).ok_or(Error::State)?;
        let step = view.list().input(host, input)?;
        if let ItemStep::Chosen(index) = step
            && let Some(target) = view.target(index)
        {
            self.chosen(host, target)?;
        }
        Ok(ItemList::handled(step))
    }

    /// A row of the menu chosen.
    fn chosen(&mut self, host: &Host<'_>, target: Target) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        match target {
            Target::Update => open_with(host, ui, CARD, &[]),
            Target::Save(deck) => {
                let mut digits = [0u8; 20];
                let id = text::decimal(deck as usize, &mut digits);
                open_with(host, ui, CARD, &[(native_sd::SAVE, id)])
            }
            Target::Mirror(deck) => {
                with_both(host, |storage| {
                    let on = sync::sources(storage)
                        .ok()
                        .and_then(|ledger| sync::link_of(&ledger, deck).map(|link| link.mirror))
                        .unwrap_or(false);
                    sync::set_mirror(storage, deck, !on);
                    Ok(())
                })?;
                if let Some(view) = self.sd_menus.top(SD_MENU_TOKEN) {
                    menu_view(host, Some(view))?;
                }
                CommandSender::new(host, &mut [0; 64]).queue_redraw(RedrawMode::Partial)
            }
        }
    }

    /// The flash cards opened: the files that are new or changed are read, as
    /// the setting says (once, as the screen opens).
    pub(crate) fn auto_update(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let ui = self.ui.as_ref().ok_or(Error::State)?;
        open_with(host, ui, CARD, &[(native_sd::AUTO, "1")])
    }
}
