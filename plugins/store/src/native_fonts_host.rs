//! *Discover fonts* on the host: reads what the device has of the fonts
//! browser, carries out the step `native_fonts::decide` names and the answer to
//! its question.
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, PluginEnabled},
    events::{AnswerOutcome, Answered},
};

use crate::native_ask::{Say as _, Words};
use crate::native_fonts::{self, NAME_KEY, PLUGIN, Step, Then};
use crate::native_glue;
use crate::native_plugin::NativeStore;
use crate::native_screen_list::FONTS;
use crate::native_screens::ids;

impl NativeStore {
    /// *Discover fonts* was run.
    pub(crate) fn discover_fonts(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.ensure_snapshot(host)?;
        let step = {
            let snapshot = self.snapshot.as_ref().ok_or(Error::State)?;
            let has_action = self.activity.actions.iter().any(|action| action == FONTS);
            let row = snapshot.rows().iter().find(|row| row.id == PLUGIN);
            native_fonts::decide(has_action, row)
        };
        let done = match step {
            Step::Run => {
                let mut scratch = [0; 128];
                CommandSender::new(host, &mut scratch).run_action(FONTS)
            }
            Step::Page => self.fonts_page(host),
            Step::Ask(words) => self.fonts_ask(host, &words),
        };
        self.release_idle();
        done
    }

    /// The store's page of the fonts browser, titled while it is fetched.
    fn fonts_page(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let title = Words::new(host)?.text(NAME_KEY)?;
        let params = [("id", native_glue::own(PLUGIN)?), ("title", title)];
        let ui = self.ui(host)?;
        native_glue::open(host, ui, ids::PLUGIN, &params)
    }

    fn fonts_ask(&mut self, host: &Host<'_>, words: &native_fonts::Words) -> Result<(), Error> {
        let texts = native_fonts::texts(&mut Words::new(host)?, words)?;
        let request = {
            let ui = self.ui(host)?;
            native_glue::confirm(host, ui, texts.question()?)?
        };
        self.fonts = Some((request, words.then));
        Ok(())
    }

    /// The reader answered the question: a yes does what it said.
    pub(crate) fn fonts_answered(
        &mut self,
        host: &Host<'_>,
        answer: &Answered,
    ) -> Result<(), Error> {
        if answer.outcome() == AnswerOutcome::Typing {
            return Ok(());
        }
        let Some((_, then)) = self.fonts.take() else {
            return Ok(());
        };
        if answer.outcome() != AnswerOutcome::Answered || answer.choice() != 1 {
            return Ok(());
        }
        let mut scratch = [0; 128];
        let mut commands = CommandSender::new(host, &mut scratch);
        match then {
            Then::Page => self.fonts_page(host),
            Then::On => {
                commands.set_plugin_enabled(PLUGIN, PluginEnabled::On)?;
                commands.restart()
            }
            Then::Restart => commands.restart(),
        }
    }
}
