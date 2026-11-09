//! `core.net` of interface 7 (`docs/abi/network.md`): the HTTP client over
//! the requests of `http` (`Calls`, an instance of their own beside the
//! interface-6 one), and the answers back through [`NetReports`].
//!
//! A request arrives as an [`HttpJob`] whose `save_path` is the staged file
//! core made (empty): the client only appends to it, core renames it, and
//! core removes it when the request ends otherwise. A redirect from `http` to
//! `https` closes the plain connection and parks the request
//! ([`NetReports::redirected_to_https`]); core starts it again when the one
//! TLS session is its, and the request goes on with the address, redirects
//! and headers it had. `ERR_NOT_FOUND` for an id core does not know is a
//! late report, not a failure of the request.
//!
//! The answer's body is allocated fallibly and cut at what the request
//! reserved from the network pool: `max_body`, or 4,096 bytes for a request
//! that saves.
//!
//! The cable and plugin servers are not served yet: `listen` refuses.

use alloc::string::String;
use alloc::vec::Vec;

use paperpaw_core::HttpCall;
use paperpaw_core::native_host::shell_wiring::{
    HttpDone, HttpJob, HttpReason, NetReports, ServedReply, ServerReason, ShellHttp,
};
use paperpaw_core::sdk::HttpError;
use paperpaw_native::host_glue::{ERR_BUSY, ERR_INVALID, ERR_NO_MEMORY, ERR_NOT_FOUND};

use super::{Bridge, Inner, Out, own, push_out};
use crate::net::Net;

/// What a request's body may take when it saves to a file: the error's body
/// kept as its reason (`http::ERROR_BODY`).
const SAVING_BODY: usize = 4096;

/// What the requests came to in one step, before it is reported.
pub(super) enum NetOut {
    /// The request ended.
    /// With the most body bytes it reserved, once taken (kept over a retry).
    Finished(u32, paperpaw_core::HttpResponse, Option<usize>),
    /// Bytes of a 2xx answer arrived.
    Progress(u32, u64, Option<u64>),
    /// The request was redirected to `https` and parked.
    Parked(u32),
}

impl Inner {
    /// Request `id` is dropped at the next step. No room to say so ends every
    /// request at the next step (the network is told lost), never a crash.
    pub(super) fn abort_later(&mut self, id: u32) {
        if self.aborts.try_reserve(1).is_ok() {
            self.aborts.push(id);
        } else {
            self.oom = true;
        }
    }

    /// Runs the requests a step on `net`; their outcomes into `out`.
    pub(super) fn http_online(
        &mut self,
        net: &mut Net,
        clock_ready: bool,
        out: &mut Vec<Out>,
    ) -> bool {
        for id in core::mem::take(&mut self.aborts) {
            self.http.abort(id, Some(net));
            self.caps.retain(|(cap, _)| *cap != id);
        }
        let Some(storage) = self.storage.clone() else {
            return false;
        };
        let answers = self.http.poll(Some(net), storage.as_ref(), clock_ready);
        for answer in answers {
            let item = Out::Net(NetOut::Finished(answer.id, answer, None));
            push_out(out, item, &mut self.oom);
        }
        for (id, received, total) in self.http.progress() {
            let item = Out::Net(NetOut::Progress(id, received, total));
            push_out(out, item, &mut self.oom);
        }
        for id in self.http.take_parked() {
            push_out(out, Out::Net(NetOut::Parked(id)), &mut self.oom);
        }
        self.http_was = self.http.busy();
        self.http.take_restart()
    }

    /// No network: what was going on is dropped, and core hears the network
    /// is gone, once.
    pub(super) fn http_offline(&mut self, net: Option<&mut Net>) {
        let had = self.http.busy() || !self.aborts.is_empty() || self.http_was;
        self.http.clear(net);
        self.aborts.clear();
        self.caps.clear();
        self.http_was = false;
        if had {
            // A flag: nothing is allocated to say it.
            self.lost.raise_requests();
        }
    }
}

impl Bridge {
    /// One report of the requests; `Err` with the report for a refusal core
    /// made with `ERR_BUSY` (it is made again on the next step).
    pub(super) fn report_net(&self, net: &NetReports, item: NetOut) -> Result<(), NetOut> {
        match item {
            NetOut::Progress(id, received, total) => again(
                net.progress(id, 200, received, total),
                NetOut::Progress(id, received, total),
            ),
            NetOut::Parked(id) => match net.redirected_to_https(id) {
                Err(ERR_BUSY) => Err(NetOut::Parked(id)),
                // Core does not know it or does not run it: it is not left
                // parked for ever (a late report is no failure of the request).
                Err(status) => {
                    if status != ERR_NOT_FOUND {
                        log::warn!("http: request {id} not resumed ({status})");
                    }
                    self.forget_request(id);
                    Ok(())
                }
                Ok(()) => Ok(()),
            },
            NetOut::Finished(id, mut response, cap) => {
                let cap = cap.unwrap_or_else(|| self.take_cap(id));
                match finish(net, id, &mut response, cap) {
                    // Core is busy: the answer is told again at the next step.
                    Told::Again => Err(NetOut::Finished(id, response, Some(cap))),
                    Told::Spent => {
                        // The body went with the refused report: counted
                        // (logged again once idle); core's own timeout ends
                        // the request.
                        log::warn!("http: the answer of request {id} was lost (core busy)");
                        self.count_lost_answer();
                        Ok(())
                    }
                    Told::Done => Ok(()),
                }
            }
        }
    }

    /// An answer that could not be told, counted (logged once idle).
    fn count_lost_answer(&self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.lost_answers = inner.lost_answers.saturating_add(1);
        }
    }

    /// Drops what is kept of request `id` (core has none of it).
    fn forget_request(&self, id: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.abort_later(id);
        }
    }

    /// The most body bytes request `id` reserved, once.
    fn take_cap(&self, id: u32) -> usize {
        self.inner
            .try_borrow_mut()
            .ok()
            .and_then(|mut inner| {
                let at = inner.caps.iter().position(|(cap, _)| *cap == id)?;
                Some(inner.caps.swap_remove(at).1)
            })
            .unwrap_or(SAVING_BODY)
    }
}

fn again(result: Result<(), i32>, item: NetOut) -> Result<(), NetOut> {
    if result == Err(ERR_BUSY) {
        Err(item)
    } else {
        Ok(())
    }
}

/// How the report of an answer went.
enum Told {
    /// Core took it (or does not know the request: a late report).
    Done,
    /// Core was busy and the answer has no body: tell it again.
    Again,
    /// Core was busy and the answer's body went with the report: lost.
    Spent,
}

/// Tells core how request `id` ended; the body is cut to what it reserved.
/// An answer without a body is told again where core is busy; one with a
/// body goes with the report, so a busy refusal loses it ([`Told::Spent`]).
fn finish(
    net: &NetReports,
    id: u32,
    response: &mut paperpaw_core::HttpResponse,
    cap: usize,
) -> Told {
    let reason = match response.error {
        None => HttpReason::Ok,
        Some(HttpError::NoNetwork) => HttpReason::NoNetwork,
        Some(HttpError::Dns) => HttpReason::Dns,
        Some(HttpError::Timeout) => HttpReason::Timeout,
        Some(HttpError::Refused) if too_large(&response.body) => HttpReason::TooLarge,
        Some(HttpError::Refused) => HttpReason::Refused,
        Some(_) => HttpReason::Io,
    };
    // A failure's text is for the shell's log, not a body.
    if response.status == 0 {
        response.body = Vec::new();
    }
    let mut done = HttpDone::new(u32::from(response.status), reason);
    done.saved = response.saved;
    done.truncated = response.truncated;
    done.etag = response.etag.as_deref().unwrap_or_default();
    done.content_type = response.content_type.as_deref().unwrap_or_default();
    done.final_url = response.url.as_deref().unwrap_or_default();
    // The body goes once, as it is: no copy of up to `max_body` bytes is
    // made for a retry (it costs heap on every request, for a refusal that
    // cannot happen between the kernel's calls).
    let kept = response.body.is_empty();
    done.body = core::mem::take(&mut response.body);
    done.body.shrink_to(cap);
    match net.finished(id, done) {
        Err(ERR_BUSY) if kept => Told::Again,
        Err(ERR_BUSY) => Told::Spent,
        _ => Told::Done,
    }
}

/// Whether a failure's text says the answer was too large.
fn too_large(text: &[u8]) -> bool {
    text.starts_with(b"too large") || text.starts_with(b"the answer is too large")
}

/// The job's headers as owned pairs, or `ERR_NO_MEMORY`.
fn owned_headers(job: &HttpJob<'_>) -> Result<Vec<(String, String)>, i32> {
    let mut out = Vec::new();
    for (name, value) in job.headers.clone() {
        out.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        out.push((own(name)?, own(value)?));
    }
    Ok(out)
}

impl ShellHttp for Bridge {
    fn start(&self, job: &HttpJob<'_>) -> Result<(), i32> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| ERR_BUSY)?;
        // The second start of a request that was redirected to `https`: it
        // goes on as it was parked.
        if inner.http.resume(job.id) {
            return Ok(());
        }
        let body = match job.body {
            Some(bytes) => {
                let mut copy = Vec::new();
                copy.try_reserve_exact(bytes.len())
                    .map_err(|_| ERR_NO_MEMORY)?;
                copy.extend_from_slice(bytes);
                Some(copy)
            }
            None => None,
        };
        let saving = job.save_path.is_some();
        let call = HttpCall {
            id: job.id,
            method: own(job.method)?,
            url: own(job.url)?,
            headers: owned_headers(job)?,
            body,
            body_from: job.send_path.map(own).transpose()?,
            max_response: if saving {
                usize::try_from(job.max_file).unwrap_or(usize::MAX)
            } else {
                job.max_body as usize
            },
            timeout_ms: job.timeout_ms,
            want_headers: Vec::new(),
            save_to: job.save_path.map(own).transpose()?,
            progress: true,
        };
        inner.caps.try_reserve(1).map_err(|_| ERR_NO_MEMORY)?;
        inner.caps.push((
            job.id,
            if saving {
                SAVING_BODY
            } else {
                job.max_body as usize
            },
        ));
        inner.http.add_staged(call);
        Ok(())
    }

    fn abort(&self, id: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.abort_later(id);
        }
    }

    fn listen(&self, _server: u32, _port: u32, _address: &mut [u8]) -> Result<usize, ServerReason> {
        Err(ServerReason::Refused)
    }

    fn unlisten(&self, _server: u32) {}

    fn reply(&self, _request: u32, _reply: &ServedReply<'_>) -> Result<(), i32> {
        Err(ERR_INVALID)
    }

    fn close_served(&self, _request: u32, _status: u32) {}
}
