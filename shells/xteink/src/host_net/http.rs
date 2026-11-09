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

use super::{Bridge, Inner, Out, own};
use crate::net::Net;

/// What a request's body may take when it saves to a file: the error's body
/// kept as its reason (`http::ERROR_BODY`).
const SAVING_BODY: usize = 4096;

/// What the requests came to in one step, before it is reported.
pub(super) enum NetOut {
    /// The request ended.
    Finished(u32, paperpaw_core::HttpResponse),
    /// Bytes of a 2xx answer arrived.
    Progress(u32, u64, Option<u64>),
    /// The request was redirected to `https` and parked.
    Parked(u32),
    /// The network went: every request core holds ends.
    Lost,
}

impl Inner {
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
            out.push(Out::Net(NetOut::Finished(answer.id, answer)));
        }
        for (id, received, total) in self.http.progress() {
            out.push(Out::Net(NetOut::Progress(id, received, total)));
        }
        for id in self.http.take_parked() {
            out.push(Out::Net(NetOut::Parked(id)));
        }
        self.http.take_restart()
    }

    /// No network: what was going on is dropped, and core hears the network
    /// is gone, once.
    pub(super) fn http_offline(&mut self, net: Option<&mut Net>, out: &mut Vec<Out>) {
        let had = self.http.busy() || !self.aborts.is_empty() || self.http_was;
        self.http.clear(net);
        self.aborts.clear();
        self.caps.clear();
        self.http_was = false;
        if had {
            out.push(Out::Net(NetOut::Lost));
        }
    }
}

impl Bridge {
    /// One report of the requests; `Err` with the report for a refusal core
    /// made with `ERR_BUSY` (it is made again on the next step).
    pub(super) fn report_net(&self, net: &NetReports, item: NetOut) -> Result<(), NetOut> {
        match item {
            NetOut::Lost => again(net.network_lost(), NetOut::Lost),
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
            NetOut::Finished(id, response) => {
                let cap = self.take_cap(id);
                let result = finish(net, id, response, cap);
                if result == Err(ERR_BUSY) {
                    // The answer is spent; core's own timeout ends the request.
                    log::warn!("http: the answer of request {id} was not taken (busy)");
                }
                Ok(())
            }
        }
    }

    /// Drops what is kept of request `id` (core has none of it).
    fn forget_request(&self, id: u32) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.aborts.push(id);
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

/// Tells core how request `id` ended; the body is cut to what it reserved.
fn finish(
    net: &NetReports,
    id: u32,
    response: paperpaw_core::HttpResponse,
    cap: usize,
) -> Result<(), i32> {
    let paperpaw_core::HttpResponse {
        status,
        mut body,
        etag,
        content_type,
        url,
        saved,
        truncated,
        error,
        ..
    } = response;
    let reason = match error {
        None => HttpReason::Ok,
        Some(HttpError::NoNetwork) => HttpReason::NoNetwork,
        Some(HttpError::Dns) => HttpReason::Dns,
        Some(HttpError::Timeout) => HttpReason::Timeout,
        Some(HttpError::Refused) if too_large(&body) => HttpReason::TooLarge,
        Some(HttpError::Refused) => HttpReason::Refused,
        Some(_) => HttpReason::Io,
    };
    let mut done = HttpDone::new(u32::from(status), reason);
    done.saved = saved;
    done.truncated = truncated;
    done.etag = etag.as_deref().unwrap_or_default();
    done.content_type = content_type.as_deref().unwrap_or_default();
    done.final_url = url.as_deref().unwrap_or_default();
    // A failure's text is for the shell's log, not a body.
    if status == 0 {
        body = Vec::new();
    } else {
        body.shrink_to(cap);
    }
    done.body = body;
    net.finished(id, done)
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
            inner.aborts.push(id);
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
