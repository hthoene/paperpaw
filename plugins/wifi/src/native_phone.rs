//! Setting up Wi-Fi with a phone, without the host: the decisions of the
//! interface-6 screen (`phone.rs`, whose module documentation says what it
//! does) as a state that is fed what happened and answers what to do, with
//! fallible allocation. The native screen turns the answers into commands
//! (`wifi_on`, `wifi_scan`, `wifi_host`, `wifi_join`, `wifi_leave`, the
//! `core.net` server and its responses) and draws; it reads the words, the
//! networks found and the clock from the host.
//!
//! Differences from interface 6: the clock is the host's milliseconds; a
//! password typed on the page lives in a [`Password`], wiped when dropped;
//! and every method that can run out of memory changes nothing when it
//! says [`Error::NoMemory`].
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::text::copy;

use crate::native_policy::Password;

pub mod page;
use page::{chosen, field};

/// The page's port: the address alone opens it.
pub const PORT: u16 = 80;
/// How long after the page answered the device leaves its own network, so
/// that the answer reaches the phone first.
pub const LEAVE_MS: u64 = 1500;
/// How long "Connected" shows before the screen closes.
pub const DONE_MS: u64 = 2500;
/// Longest form taken, in bytes.
pub const MAX_FORM: usize = 1024;
/// Without a request from the phone this long, the own network closes (and
/// the screen with it): it is not left open by mistake.
pub const IDLE_MS: u64 = 10 * 60 * 1000;
/// Letters of the own network's password: 62 bits, three groups of five.
pub const PASSWORD_LETTERS: usize = 15;

/// The step shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The phone joins the own network.
    Join,
    /// The phone opens the page.
    Open,
}

impl Step {
    const fn other(self) -> Self {
        match self {
            Self::Join => Self::Open,
            Self::Open => Self::Join,
        }
    }
}

/// Why nothing goes on.
#[derive(Debug, PartialEq, Eq)]
pub enum Failed {
    /// The page could not be served, for this reason.
    Server(String),
    /// The own network could not open.
    NotOpened,
}

/// Where the setting up is. A password in it prints as `Password(..)`.
#[derive(Debug)]
pub enum Stage {
    /// Looking for networks, to list them on the page.
    Searching,
    /// The own network opens.
    Opening,
    /// The own network is open; the page's address once it is served.
    Hosting {
        page: Option<String>,
    },
    /// The network chosen on the phone, joined once the answer went.
    Leaving {
        ssid: String,
        password: Password,
    },
    Joining(String),
    Joined(String),
    Failed(Failed),
}

impl Stage {
    /// Opening or open: the steps can be switched.
    const fn steps(&self) -> bool {
        matches!(self, Self::Opening | Self::Hosting { .. })
    }

    /// Left alone, these end by themselves.
    const fn waiting(&self) -> bool {
        matches!(self, Self::Opening | Self::Hosting { .. } | Self::Failed(_))
    }
}

/// What the radio says now.
#[derive(Debug, Clone, Copy, Default)]
pub struct Radio<'a> {
    /// Anything but off.
    pub on: bool,
    /// The own network is open.
    pub hosting: bool,
    /// Phones that joined the own network.
    pub guests: u32,
    /// The network the device is online in.
    pub online: Option<&'a str>,
}

/// What to show for a request (the words and the networks are the screen's).
#[derive(Debug, PartialEq, Eq)]
pub enum Page {
    /// The networks to choose from; `again` after a form without one.
    Choose { again: bool },
    /// The network being joined.
    Joining(String),
    /// Nothing is served there.
    NotFound,
}

/// What to do. The screen does them in order.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Draw the screen again.
    Redraw,
    /// What the policy is told: the radio is held on by this screen
    /// (`phone`) and `manual` is what it was or is to be again. The goal is
    /// nothing.
    Policy { manual: bool, phone: bool },
    /// Switch the radio on.
    RadioOn,
    /// Look for networks.
    Scan,
    /// Open the own network.
    Host,
    /// Serve the page (port [`PORT`]).
    Serve,
    /// Stop serving it.
    StopServing,
    /// Close the own network.
    Leave,
    /// Answer request number `request`.
    Respond {
        request: u32,
        status: u16,
        page: Page,
    },
    /// Join the network chosen on the phone (the policy decides the
    /// commands, remembers it as pending, and keeps its password once
    /// joined).
    Choose { ssid: String, password: Password },
    /// Wake the screen at this time (milliseconds); the last wake counts.
    WakeAt(u64),
    /// Leave the screen.
    Back,
}

/// What a button or key means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Back,
    Confirm,
    NextPage,
    PrevPage,
}

/// An answer with room for the most effects one call makes (four), so that
/// pushing them cannot run out of memory half way.
fn room() -> Result<Vec<Effect>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(4).map_err(|_| Error::NoMemory)?;
    Ok(out)
}

/// The phone setup.
pub struct Phone {
    /// The own network's name, if a password could be drawn for it.
    own: Option<String>,
    stage: Stage,
    step: Step,
    /// Why joining failed last, said above the codes.
    problem: Option<String>,
    /// Whether the user had switched the radio on before.
    manual: bool,
    started: bool,
    /// Drawn anew each time, in the page's form: a form sent from another
    /// page in the phone's browser does not have it.
    token: String,
    /// When the phone asked last (or the own network opened).
    active: u64,
    /// Whether a phone joined and the step went on by itself: once only.
    moved_on: bool,
}

impl core::fmt::Debug for Phone {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let stage = match &self.stage {
            Stage::Searching => "Searching",
            Stage::Opening => "Opening",
            Stage::Hosting { .. } => "Hosting",
            Stage::Leaving { .. } => "Leaving",
            Stage::Joining(_) => "Joining",
            Stage::Joined(_) => "Joined",
            Stage::Failed(_) => "Failed",
        };
        f.debug_struct("Phone")
            .field("stage", &stage)
            .field("step", &self.step)
            .field("active", &self.active)
            .field("moved_on", &self.moved_on)
            .finish_non_exhaustive()
    }
}

impl Phone {
    /// A setup at `now`: `own` is the own network's name (none where no
    /// password could be drawn), `token` the page's token, `manual` whether
    /// the user had switched the radio on before.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn new(own: Option<&str>, token: &str, manual: bool, now: u64) -> Result<Self, Error> {
        Ok(Self {
            own: own.map(copy).transpose()?,
            stage: Stage::Searching,
            step: Step::Join,
            problem: None,
            manual,
            started: false,
            token: copy(token)?,
            active: now,
            moved_on: false,
        })
    }

    /// The stage.
    #[must_use]
    pub const fn stage(&self) -> &Stage {
        &self.stage
    }

    /// The step shown.
    #[must_use]
    pub const fn step(&self) -> Step {
        self.step
    }

    /// Why joining failed last.
    #[must_use]
    pub fn problem(&self) -> Option<&str> {
        self.problem.as_deref()
    }

    /// The page's token.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The page's address, once it is served.
    #[must_use]
    pub fn page_address(&self) -> Option<&str> {
        match &self.stage {
            Stage::Hosting { page } => page.as_deref(),
            _ => None,
        }
    }

    /// The screen is shown (once counts): the radio is held on while it is
    /// open, and it looks for networks.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn shown(&mut self, radio_on: bool) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        if self.started {
            return Ok(out);
        }
        if self.own.is_none() {
            self.started = true;
            out.push(Effect::Redraw);
            return Ok(out);
        }
        for effect in [
            Effect::Policy {
                manual: true,
                phone: true,
            },
            Effect::RadioOn,
            Effect::Scan,
        ] {
            if matches!(effect, Effect::RadioOn) && radio_on {
                continue;
            }
            out.push(effect);
        }
        self.started = true;
        Ok(out)
    }

    /// The scan came back: the own network opens.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn scanned(&mut self) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        if matches!(self.stage, Stage::Searching) {
            out.push(Effect::Redraw);
            if self.own.is_some() {
                out.push(Effect::Host);
                self.stage = Stage::Opening;
            }
        }
        Ok(out)
    }

    /// The radio's state changed.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn radio_changed(&mut self, now: u64, radio: &Radio<'_>) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        match &self.stage {
            Stage::Opening if radio.hosting => {
                out.push(Effect::Serve);
                out.push(Effect::WakeAt(now.saturating_add(IDLE_MS)));
                out.push(Effect::Redraw);
                self.stage = Stage::Hosting { page: None };
                self.active = now;
                if radio.guests > 0 {
                    self.moved_on = true;
                    self.step = Step::Open;
                }
            }
            // A phone joined: on to opening the page, once.
            Stage::Hosting { .. } if radio.hosting && radio.guests > 0 && !self.moved_on => {
                out.push(Effect::WakeAt(now.saturating_add(IDLE_MS)));
                if self.step != Step::Open {
                    out.push(Effect::Redraw);
                }
                self.moved_on = true;
                self.active = now;
                self.step = Step::Open;
            }
            Stage::Joining(ssid) if radio.online == Some(ssid.as_str()) => {
                let joined = copy(ssid)?;
                out.push(Effect::WakeAt(now.saturating_add(DONE_MS)));
                out.push(Effect::Redraw);
                self.stage = Stage::Joined(joined);
            }
            _ => {}
        }
        Ok(out)
    }

    /// A join or the own network failed; `problem` says why in words.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn failed(&mut self, now: u64, ssid: &str, problem: &str) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        // The own network could not open.
        if matches!(self.stage, Stage::Opening) && self.own.as_deref() == Some(ssid) {
            out.push(Effect::WakeAt(now.saturating_add(IDLE_MS)));
            out.push(Effect::Redraw);
            self.stage = Stage::Failed(Failed::NotOpened);
            self.active = now;
            return Ok(out);
        }
        if !matches!(&self.stage, Stage::Joining(joining) if joining == ssid) {
            return Ok(out);
        }
        let problem = copy(problem)?;
        if self.own.is_some() {
            out.push(Effect::Host);
        }
        out.push(Effect::Redraw);
        if self.own.is_some() {
            self.stage = Stage::Opening;
        }
        self.problem = Some(problem);
        Ok(out)
    }

    /// The page is served at `address`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn server_started(&mut self, address: &str) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        if let Stage::Hosting { .. } = self.stage {
            // Without the port where it is the usual one.
            let address = address.strip_suffix(":80").unwrap_or(address);
            let url = paperpaw_ui_kit::text::concat(&["http://", address, "/"])?;
            out.push(Effect::Redraw);
            self.stage = Stage::Hosting { page: Some(url) };
        }
        Ok(out)
    }

    /// The page could not be served, for `reason`.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn server_failed(&mut self, reason: &str) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        let reason = copy(reason)?;
        out.push(Effect::Redraw);
        self.stage = Stage::Failed(Failed::Server(reason));
        Ok(out)
    }

    /// A request came for the page: `form` is its body, if it had one (one
    /// over [`MAX_FORM`] bytes counts as none). The body can hold a password:
    /// the caller wipes it once this returns.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn request(
        &mut self,
        now: u64,
        request: u32,
        method: &str,
        target: &str,
        form: Option<&str>,
    ) -> Result<Vec<Effect>, Error> {
        let path = target.split_once('?').map_or(target, |(path, _)| path);
        // A body over the most taken is none: the interface-6 screen read at most
        // that much and took a form it could not read for none.
        let form = form.filter(|form| form.len() <= MAX_FORM);
        let token = self.token.as_str();
        let (status, page, picked) = match (method, path) {
            // Only the page's own form: not one another page sent.
            ("POST", "/join")
                if token.is_empty()
                    || form
                        .map(|f| field(f, "t"))
                        .transpose()?
                        .flatten()
                        .as_deref()
                        != Some(token) =>
            {
                (403, Page::Choose { again: true }, None)
            }
            ("POST", "/join") => match form.map(chosen).transpose()?.flatten() {
                Some((ssid, password)) => {
                    (200, Page::Joining(copy(&ssid)?), Some((ssid, password)))
                }
                None => (400, Page::Choose { again: true }, None),
            },
            ("GET", "/") => (200, Page::Choose { again: false }, None),
            _ => (404, Page::NotFound, None),
        };
        let mut out = room()?;
        out.push(Effect::WakeAt(now.saturating_add(IDLE_MS)));
        out.push(Effect::Respond {
            request,
            status,
            page,
        });
        if let Some((ssid, password)) = picked {
            out.push(Effect::WakeAt(now.saturating_add(LEAVE_MS)));
            out.push(Effect::Redraw);
            self.stage = Stage::Leaving { ssid, password };
        }
        self.active = now;
        Ok(out)
    }

    /// The screen's timer fired.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn timer(&mut self, now: u64, radio: &Radio<'_>) -> Result<Vec<Effect>, Error> {
        // Left alone: closed.
        if self.stage.waiting() && now >= self.active.saturating_add(IDLE_MS) {
            return self.close(radio);
        }
        let mut out = room()?;
        match &self.stage {
            Stage::Leaving { ssid, .. } => {
                // Everything that can fail first; then the state moves on.
                let joining = copy(ssid)?;
                let left = core::mem::replace(&mut self.stage, Stage::Joining(joining));
                if let Stage::Leaving { ssid, password } = left {
                    out.push(Effect::StopServing);
                    out.push(Effect::Choose { ssid, password });
                    out.push(Effect::Redraw);
                }
            }
            Stage::Joined(_) => return self.close(radio),
            _ => {}
        }
        // The screen's one timer: the idle time again, where it counts.
        if self.stage.waiting() {
            out.push(Effect::WakeAt(self.active.saturating_add(IDLE_MS)));
        }
        Ok(out)
    }

    /// A button or key. `None` where it is not the setup's.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn action(
        &mut self,
        action: Action,
        radio: &Radio<'_>,
    ) -> Result<Option<Vec<Effect>>, Error> {
        let to = match action {
            Action::Back => return self.close(radio).map(Some),
            Action::Confirm if self.stage.steps() => self.step.other(),
            Action::NextPage if self.stage.steps() => Step::Open,
            Action::PrevPage if self.stage.steps() => Step::Join,
            _ => return Ok(None),
        };
        let mut out = room()?;
        if self.step != to {
            out.push(Effect::Redraw);
            self.step = to;
        }
        Ok(Some(out))
    }

    /// Closes the screen: the page no longer served, the own network closed,
    /// the radio as the user left it.
    fn close(&mut self, radio: &Radio<'_>) -> Result<Vec<Effect>, Error> {
        let mut out = room()?;
        out.push(Effect::StopServing);
        if radio.hosting {
            out.push(Effect::Leave);
        }
        out.push(Effect::Policy {
            manual: self.manual,
            phone: false,
        });
        out.push(Effect::Back);
        Ok(out)
    }
}

#[cfg(test)]
mod tests;
