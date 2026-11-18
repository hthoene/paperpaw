//! The requests of `core.net`: the states (the whole list saved to a file of
//! the data folder, whatever its length, and read back in pieces), one state,
//! and a service call; and the small services around them: whether Wi-Fi is up
//! already (the radio is never switched on for this plugin's own fetching) and
//! the work that fetches while charging.
//!
//! The state of a request is held here (which one is under way, and what it
//! was for); what an answer means to the entities is the plugin's.
use crate::{
    declarations::WORK_TOKEN,
    keep::Secret,
    link::{self, Problem},
    model::valid_id,
    states::{self, Record},
};
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::{
    Error, Host,
    device::{Device, WifiState},
    net::{
        BODY_PART, FileRoot, Header, HttpDone, Net, REPLY, REQUEST_INPUT, Reason, Request,
        RequestId,
    },
    storage::{self, Root},
    work::{Moment, SPAN_MAX_MS, Work, WorkSpec},
};
use paperpaw_ui_kit::mem::{self, SERVICE_BUFFER};

/// The plugin's own numbers of the requests, in their `http_done` events.
pub const REQ_STATES: u32 = 1;
pub const REQ_CALL: u32 = 2;
pub const REQ_ONE: u32 = 3;
/// Where the list of all states is saved while it is read.
pub const STATES_FILE: &str = "states.json";
/// The key of the work that fetches while charging.
pub const WORK_KEY: &str = "smart-home.refresh";
/// The longest list saved, in bytes.
const MAX_FILE: u64 = 8 << 20;
/// The longest answer to a service call or a single state kept in memory.
const MAX_BODY: u32 = 6 * 1024;
/// The `User-Agent` sent.
const USER_AGENT: &str = "PaperPaw/0.6 (e-reader)";
/// A fetch under way is not asked again for this long, in milliseconds.
const PATIENCE_MS: u64 = 90_000;
/// How long a request may take, in milliseconds.
const TIMEOUT_MS: u32 = 30_000;

/// Why the states are fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// On its own (a start, a wake, the schedule): a failure is recorded, not shown.
    Quiet,
    /// The dashboard was opened: fetched wherever the network stands; a failure
    /// is shown in its note, not told.
    Open,
    /// The reader asked: a failure is told.
    Asked,
    /// The picker is open and needs the whole list kept in the file.
    Picker,
}

/// What a request in flight is.
#[derive(Debug)]
enum Flight {
    States(Why),
    /// A service call for this entity.
    Call(String),
    /// A single state read for this entity (after a call).
    One(String),
}

/// A request that has ended, told apart.
#[derive(Debug)]
#[non_exhaustive]
pub enum Ended {
    /// The list of states: why it was fetched.
    States(Why),
    /// The service call for this entity.
    Call(String),
    /// The single state of this entity.
    One(String),
}

/// The handles of the services and the requests in flight.
#[derive(Debug, Default)]
pub struct Fetcher {
    net: Option<Net>,
    work: Option<Work>,
    device: Option<Device>,
    flights: Vec<(RequestId, Flight)>,
    /// The list was asked for at this moment (the host's clock, milliseconds).
    asked: Option<u64>,
}

impl Fetcher {
    fn net(&mut self, host: &Host<'_>) -> Result<&Net, Error> {
        if self.net.is_none() {
            self.net = Some(Net::open(host)?);
        }
        self.net.as_ref().ok_or(Error::State)
    }

    fn work(&mut self, host: &Host<'_>) -> Result<&Work, Error> {
        if self.work.is_none() {
            self.work = Some(Work::open(host)?);
        }
        self.work.as_ref().ok_or(Error::State)
    }

    fn device(&mut self, host: &Host<'_>) -> Result<&Device, Error> {
        if self.device.is_none() {
            self.device = Some(Device::open(host)?);
        }
        self.device.as_ref().ok_or(Error::State)
    }

    /// Whether the list is being fetched (and so is not asked for again).
    #[must_use]
    pub fn fetching(&self, now: u64) -> bool {
        self.asked
            .is_some_and(|at| now.saturating_sub(at) < PATIENCE_MS)
            && self
                .flights
                .iter()
                .any(|(_, flight)| matches!(flight, Flight::States(_)))
    }

    /// Whether a service call for `id` is under way.
    #[must_use]
    pub fn calling(&self, id: &str) -> bool {
        self.flights
            .iter()
            .any(|(_, f)| matches!(f, Flight::Call(e) | Flight::One(e) if e == id))
    }

    /// Whether Wi-Fi is up already: where PaperPaw manages the radio, only while
    /// it is in a network (the radio is not switched on for a fetch of its own).
    pub fn online(&mut self, host: &Host<'_>) -> bool {
        let (Ok(mut input), Ok(mut out)) =
            (mem::buffer(SERVICE_BUFFER), mem::buffer(SERVICE_BUFFER))
        else {
            return false;
        };
        let Ok(device) = self.device(host) else {
            return self.net(host).is_ok();
        };
        match device.wifi(host, &mut input, &mut out) {
            Ok(info) => matches!(info.state(), WifiState::Online | WifiState::Hosting),
            // No radio of PaperPaw's: a network is there when the service is.
            Err(Error::NotFound | Error::Unsupported) => self.net(host).is_ok(),
            Err(_) => false,
        }
    }

    fn start(
        &mut self,
        host: &Host<'_>,
        request: &Request<'_>,
        flight: Flight,
    ) -> Result<(), Error> {
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        self.flights.try_reserve(1).map_err(|_| Error::NoMemory)?;
        let id = self.net(host)?.request(host, request, &mut input, &mut out);
        mem::wipe(&mut input);
        self.flights.push((id?, flight));
        Ok(())
    }

    /// Asks for all states, saved to [`STATES_FILE`].
    ///
    /// # Errors
    /// What the request is refused for; `NoMemory`.
    pub fn send_states(
        &mut self,
        host: &Host<'_>,
        base: &str,
        token: &Secret,
        why: Why,
        now: u64,
    ) -> Result<(), Error> {
        let url = link::states_url(base)?;
        let headers = [
            Header::new("Authorization", token.as_str())?,
            Header::new("Accept", "application/json")?,
            Header::new("User-Agent", USER_AGENT)?,
        ];
        let request = Request::get(REQ_STATES, &url)?
            .headers(&headers)?
            .save_to(FileRoot::Data, STATES_FILE)?
            .max_file(MAX_FILE)?
            .max_body(16)
            .timeout_ms(TIMEOUT_MS)?;
        self.start(host, &request, Flight::States(why))?;
        self.asked = Some(now);
        Ok(())
    }

    /// Calls the service that switches `id`.
    ///
    /// # Errors
    /// `Invalid` for an id that is not one; what the request is refused for.
    pub fn send_call(
        &mut self,
        host: &Host<'_>,
        base: &str,
        token: &Secret,
        id: &str,
    ) -> Result<(), Error> {
        let service = crate::model::service_of(id).ok_or(Error::Invalid)?;
        if !valid_id(id) {
            return Err(Error::Invalid);
        }
        let url = link::service_url(base, crate::model::domain(id), service)?;
        let body = link::service_body(id)?;
        let headers = [
            Header::new("Authorization", token.as_str())?,
            Header::new("Content-Type", "application/json")?,
            Header::new("Accept", "application/json")?,
            Header::new("User-Agent", USER_AGENT)?,
        ];
        let request = Request::new(REQ_CALL, "POST", &url)?
            .headers(&headers)?
            .body(body.as_bytes())?
            .max_body(MAX_BODY)
            .timeout_ms(15_000)?;
        let flight = Flight::Call(crate::model::clean(id, crate::model::ID_MAX)?);
        self.start(host, &request, flight)
    }

    /// Asks for the single state of `id`.
    ///
    /// # Errors
    /// As [`Self::send_call`].
    pub fn send_one(
        &mut self,
        host: &Host<'_>,
        base: &str,
        token: &Secret,
        id: &str,
    ) -> Result<(), Error> {
        if !valid_id(id) {
            return Err(Error::Invalid);
        }
        let url = link::state_url(base, id)?;
        let headers = [
            Header::new("Authorization", token.as_str())?,
            Header::new("Accept", "application/json")?,
            Header::new("User-Agent", USER_AGENT)?,
        ];
        let request = Request::get(REQ_ONE, &url)?
            .headers(&headers)?
            .max_body(MAX_BODY)
            .timeout_ms(15_000)?;
        let flight = Flight::One(crate::model::clean(id, crate::model::ID_MAX)?);
        self.start(host, &request, flight)
    }

    /// The request that ended, told apart; `None` for one that is not under way
    /// (cancelled, replaced).
    pub fn ended(&mut self, done: &HttpDone) -> Option<Ended> {
        let at = self.flights.iter().position(|(id, _)| *id == done.id())?;
        let (_, flight) = self.flights.remove(at);
        if matches!(flight, Flight::States(_)) {
            self.asked = None;
        }
        if matches!(done.reason(), Reason::Cancelled | Reason::Replaced) {
            return None;
        }
        Some(match flight {
            Flight::States(why) => Ended::States(why),
            Flight::Call(id) => Ended::Call(id),
            Flight::One(id) => Ended::One(id),
        })
    }

    /// The body of the request that ended, kept in memory, in pieces.
    ///
    /// # Errors
    /// `NoMemory`, `NoSpace`; what the host says; what `sink` says.
    pub fn read_body(
        &mut self,
        host: &Host<'_>,
        done: &HttpDone,
        sink: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let length = usize::try_from(done.body_len()).map_err(|_| Error::NoSpace)?;
        let mut input = mem::buffer(REQUEST_INPUT)?;
        let mut out = mem::buffer(REPLY)?;
        let net = self.net(host)?;
        let mut offset = 0usize;
        while offset < length {
            let at = u32::try_from(offset).map_err(|_| Error::NoSpace)?;
            let part = net.body(host, done.id(), at, &mut input, &mut out)?;
            if part.is_empty() {
                break;
            }
            let take = part.len().min(BODY_PART).min(length - offset);
            sink(part.get(..take).ok_or(Error::Invalid)?)?;
            offset += take;
        }
        Ok(())
    }

    /// Declares the work that fetches `after_ms` from now (while charging), or
    /// takes it away (`None`). A timer that cannot be set costs the next fetch only.
    ///
    /// # Errors
    /// `NoMemory`; what the service says.
    pub fn schedule(&mut self, host: &Host<'_>, after_ms: Option<u32>) -> Result<(), Error> {
        let mut input = mem::buffer(256)?;
        let mut out = mem::buffer(64)?;
        let Some(after) = after_ms else {
            return self
                .work(host)?
                .cancel(host, WORK_KEY, &mut input, &mut out);
        };
        let spec = WorkSpec::when_cheap(WORK_KEY, WORK_TOKEN)?
            .after_ms(after.min(SPAN_MAX_MS))
            .at_moment(Moment::Online);
        self.work(host)?.declare(host, &spec, &mut input, &mut out)
    }
}

/// Reads the saved list from the card and gives its records to `sink`.
///
/// # Errors
/// `NoMemory`; what the card and `sink` say.
pub fn read_saved(
    host: &Host<'_>,
    sink: &mut dyn FnMut(Record) -> Result<(), Error>,
) -> Result<(), Error> {
    states::read_file(
        &mut |at, out| storage::read_at(host, Root::Data, STATES_FILE, at, out),
        sink,
    )
}

/// Removes the saved list.
pub fn drop_saved(host: &Host<'_>) {
    storage::remove(host, Root::Data, STATES_FILE).ok();
}

/// The words a failed request is told in: its [`Problem`].
#[must_use]
pub fn problem_of(done: &HttpDone) -> Option<Problem> {
    Problem::of(done.reason(), done.status())
}
