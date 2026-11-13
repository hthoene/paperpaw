//! Downloading, installing, updating and removing a font on the host:
//! each file of the font's plan comes with `core.net` into the data folder's
//! `staging/<plugin>/`, is checked against its size and SHA-1, and once all are
//! there the host is asked to install the folder (`command.install_plugin`: it
//! asks the user, decision 132); removing asks the host too (it asks the user,
//! decision 124). The ends come as `http_done` and `install_done`.
use super::{
    DOWNLOADED_FILE, FONT, INSTALL_REQUEST, LIST, NativeFontsBrowser, REMOVE_REQUEST, View,
};
use crate::{
    native_catalog::Font,
    native_job::{self, Job},
    native_plan::Plan,
    native_view::Running,
};
use paperpaw_sdk::native::{
    Error, Host,
    commands::{CommandSender, InstallRoot, RedrawMode},
    device::{DeviceOutcome, InstallKind},
    events::InstallDone,
    formatting::Arg,
    net::{FileRoot, HttpDone, REPLY, REQUEST_INPUT, Reason, Request},
    storage::DataStorage,
    ui::Notify,
};
use paperpaw_ui_kit::{Messages, mem::buffer, text::copy};

impl NativeFontsBrowser {
    /// Starts downloading `font` (as an update where one is installed). One
    /// font at a time: another is said to wait.
    pub(super) fn download(
        &mut self,
        host: &Host<'_>,
        font: &Font,
        update: bool,
    ) -> Result<(), Error> {
        if self.job.is_some() {
            return Self::say(host, "fonts-browser.busy", &font.name);
        }
        if !self.has_network(host)? {
            return Self::tell(host, "fonts-browser.offline");
        }
        // Offline mode: the user is asked first, once, in place of the rows.
        if !self.decided() && self.in_offline_mode(host)? {
            if let Some(View::Font(view)) = self.screens.top(FONT) {
                view.offer = Some(update);
            }
            return self.refresh(host);
        }
        let outlines = self.outlines(host)?;
        let Some(plan) = Plan::new(font, outlines)? else {
            return Self::say(host, "fonts-browser.failed", &font.name);
        };
        self.started = self.started.wrapping_add(1);
        let request = INSTALL_REQUEST.wrapping_add(self.started);
        // Leftovers of an earlier, cut-off download of this font go first.
        native_job::clear(&DataStorage::new(host), &plan);
        self.job = Some(Job::new(plan, copy(&font.name)?, update, request));
        if let Err(error) = self.request_file(host) {
            self.give_up(host)?;
            return if error == Error::NoMemory {
                Err(error)
            } else {
                Self::say(host, "fonts-browser.failed", &font.name)
            };
        }
        self.refresh(host)
    }

    /// Whether the user went online for the page of the font on top.
    fn decided(&mut self) -> bool {
        matches!(self.screens.top(FONT), Some(View::Font(view)) if view.decided)
    }

    /// The download going on, as the screens show it.
    pub(super) fn running(&self) -> Result<Option<Running>, Error> {
        self.job
            .as_ref()
            .map(|job| {
                Ok(Running {
                    plugin: copy(&job.plan.plugin)?,
                    done: job.done(),
                    total: job.total(),
                    installing: job.installing(),
                })
            })
            .transpose()
    }

    /// Asks for the file the job is at.
    fn request_file(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.net(host)?;
        let Some(job) = &self.job else {
            return Ok(());
        };
        let Some(step) = job.current() else {
            return Ok(());
        };
        let request = Request::get(DOWNLOADED_FILE, &step.url)?
            .save_to(FileRoot::Data, &step.save)?
            .max_file(job.plan.max_file)?;
        let (mut input, mut reply) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
        let net = self.net.as_ref().ok_or(Error::State)?;
        let id = net.request(host, &request, &mut input, &mut reply)?;
        self.file_request = Some(id);
        Ok(())
    }

    /// A file arrived, or did not: checked, and on to the next or to the install.
    pub(super) fn file_done(&mut self, host: &Host<'_>, done: &HttpDone) -> Result<(), Error> {
        if self.file_request != Some(done.id()) {
            return Ok(());
        }
        self.file_request = None;
        let arrived = done.reason() == Reason::Ok && (200..300).contains(&done.status());
        let kept = match (&self.job, arrived) {
            (Some(job), true) => match job.current() {
                Some(step) => native_job::verify(&DataStorage::new(host), step)?,
                None => false,
            },
            _ => false,
        };
        let more = match (&mut self.job, kept) {
            (Some(job), true) => job.advance(),
            _ => return self.fail(host),
        };
        if more {
            if self.request_file(host).is_err() {
                return self.fail(host);
            }
            // The pages say how far it is.
            return self.refresh(host);
        }
        self.install(host)
    }

    /// Every file is there: the host is asked to install the folder.
    fn install(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let Some(job) = &mut self.job else {
            return Ok(());
        };
        job.install_asked();
        let mut scratch = [0; 512];
        let asked = CommandSender::new(host, &mut scratch).install_plugin(
            job.request(),
            InstallRoot::Data,
            &job.plan.folder,
            job.update,
            false,
        );
        match asked {
            // The page stops offering *Stop* now.
            Ok(()) => self.refresh(host),
            Err(Error::NoMemory) => Err(Error::NoMemory),
            Err(_) => self.fail(host),
        }
    }

    /// The end of an install or a removal that this plugin asked for.
    pub(super) fn install_done(
        &mut self,
        host: &Host<'_>,
        done: &InstallDone<'_>,
    ) -> Result<(), Error> {
        let Some(request) = done.request() else {
            return Ok(());
        };
        let taken = done.status().is_ok() && done.outcome() == DeviceOutcome::Ok;
        let own = self
            .job
            .as_ref()
            .is_some_and(|job| job.ends_with(request.get()));
        match (request.get(), done.kind()) {
            (_, InstallKind::Plugin) if own => {
                let Some(job) = self.job.take() else {
                    return Ok(());
                };
                native_job::clear(&DataStorage::new(host), &job.plan);
                // Only a finished install is ready; a refusal, a dismissed or a
                // timed-out question and any failure leave nothing to use.
                let key = if taken {
                    "fonts-browser.ready"
                } else {
                    "fonts-browser.failed"
                };
                Self::say(host, key, &job.name)?;
                self.refresh(host)
            }
            (REMOVE_REQUEST, InstallKind::Removal) => self.refresh(host),
            _ => Ok(()),
        }
    }

    /// Stops the download going on (or the wait for the install's question).
    pub(super) fn stop(&mut self, host: &Host<'_>) -> Result<(), Error> {
        // The host's question is open: the job stays until `install_done`.
        if self.job.as_ref().is_some_and(Job::installing) {
            return Ok(());
        }
        if self.file_request.take().is_some() {
            let (mut input, mut out) = (buffer(REQUEST_INPUT)?, buffer(REPLY)?);
            self.net(host)?
                .cancel(host, DOWNLOADED_FILE, &mut input, &mut out)
                .ok();
        }
        self.give_up(host)?;
        self.refresh(host)
    }

    /// Asks the host to remove the plugin of `font`.
    pub(super) fn remove(&mut self, host: &Host<'_>, font: &Font) -> Result<(), Error> {
        let plugin = font.plugin()?;
        let mut scratch = [0; 256];
        match CommandSender::new(host, &mut scratch).remove_plugin(REMOVE_REQUEST, &plugin) {
            Ok(()) => Ok(()),
            Err(Error::Denied | Error::Full) => {
                Self::say(host, "fonts-browser.remove-failed", &font.name)
            }
            Err(error) => Err(error),
        }
    }

    /// The job ended badly: what was staged goes and the user is told.
    fn fail(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let name = self.job.as_ref().map(|job| copy(&job.name)).transpose()?;
        self.give_up(host)?;
        if let Some(name) = name {
            Self::say(host, "fonts-browser.failed", &name)?;
        }
        self.refresh(host)
    }

    /// Drops the job and what it staged.
    fn give_up(&mut self, host: &Host<'_>) -> Result<(), Error> {
        self.file_request = None;
        if let Some(job) = self.job.take() {
            native_job::clear(&DataStorage::new(host), &job.plan);
        }
        Ok(())
    }

    /// Says `key` as it is.
    fn tell(host: &Host<'_>, key: &str) -> Result<(), Error> {
        let text = Messages::new()?.copy(host, key)?;
        let mut scratch = [0; 384];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// Says `key` of `name`.
    fn say(host: &Host<'_>, key: &str, name: &str) -> Result<(), Error> {
        let text = Messages::new()?.format(
            host,
            key,
            &[Arg::Text {
                name: "name",
                value: name,
            }],
        )?;
        let mut scratch = [0; 384];
        CommandSender::new(host, &mut scratch).notify(Notify::new(&text)?)
    }

    /// The pages show what can be done now: the font on top, and the list's
    /// states (installed, reading, how far a download is).
    pub(super) fn refresh(&mut self, host: &Host<'_>) -> Result<(), Error> {
        let installed = self.installed(host)?;
        let running = self.running()?;
        if let Some(View::Font(view)) = self.screens.top(FONT) {
            let font = view.font.as_ref().map(Font::try_clone).transpose()?;
            let (offer, decided) = (view.offer, view.decided);
            let mut fresh =
                Self::font_view(host, font, &installed, running.as_ref(), offer.is_some())?;
            fresh.offer = offer;
            fresh.decided = decided;
            *view = fresh;
        }
        if let Some(View::List(view)) = self.screens.top(LIST)
            && !view.fonts.is_empty()
        {
            let items = Self::font_items(host, &view.fonts, &installed, running.as_ref())?;
            view.list.replace(items);
        }
        let mut scratch = [0; 64];
        CommandSender::new(host, &mut scratch).queue_redraw(RedrawMode::Partial)
    }
}
