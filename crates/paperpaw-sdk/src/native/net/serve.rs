//! Serving HTTP (`docs/abi/network.md` §5): start and stop a server, and answer
//! the requests it gets. The requests arrive as `event.served_request` events
//! and the server's state as `event.server`; this module only sends.
use super::{
    FileRoot, Header, Net, Row, check_path,
    wire::{self},
};
use crate::native::{Error, Host, wire::services_wire::core_net_v1 as schema};

/// The most inline body bytes of a response.
pub const MAX_RESPONSE_BODY: usize = super::MAX_INLINE_BODY;
/// The most headers of a response.
pub const MAX_RESPONSE_HEADERS: usize = 8;
/// The longest request body a server may be asked to take: 64 MiB.
pub const MAX_SERVED_BODY: u64 = 64 << 20;
/// The longest content type of a response.
const CONTENT_TYPE_MAX: usize = wire::SERVICE_NET_RESPONSE_CONTENT_TYPE_MAX_BYTES;

/// A server to start with [`Net::serve`]: the plugin's own number for it (the
/// `token`, any number: it only names the server in its events and in
/// [`Net::stop_serving`]), where it listens and the largest request body it takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Server {
    token: u32,
    port: u16,
    max_body: u64,
}

impl Server {
    /// A server on `port` (1-65535; the host tries the next 20 when it is
    /// taken, and the `started` event's address says which it got). Request
    /// bodies are taken up to the host's default of 4 MiB.
    ///
    /// # Errors
    /// `Invalid` for port 0: use [`Server::cable`] for the USB cable.
    pub const fn new(token: u32, port: u16) -> Result<Self, Error> {
        if port == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            token,
            port,
            max_body: 0,
        })
    }

    /// A server over the USB cable, to a computer, without a network (the X4
    /// only: elsewhere the host answers `Unsupported`).
    pub const fn cable(token: u32) -> Self {
        Self {
            token,
            port: 0,
            max_body: 0,
        }
    }

    /// The largest request body the server takes, at most [`MAX_SERVED_BODY`];
    /// 0 for the host's default of 4 MiB. A larger one is answered 413.
    ///
    /// # Errors
    /// `NoSpace` for more than 64 MiB.
    pub const fn max_body(mut self, bytes: u64) -> Result<Self, Error> {
        if bytes > MAX_SERVED_BODY {
            return Err(Error::NoSpace);
        }
        self.max_body = bytes;
        Ok(self)
    }

    /// The plugin's number for the server, echoed in its events.
    #[must_use]
    pub const fn token(&self) -> u32 {
        self.token
    }
}

/// The answer to one served request, built from borrowed parts.
#[derive(Clone, Copy)]
pub struct Response<'a> {
    request: u32,
    status: u16,
    content_type: &'a str,
    headers: &'a [Header<'a>],
    body: &'a [u8],
    send: Option<(FileRoot, &'a str)>,
}

/// `Debug` shows the sizes only: a header value may be a secret and the body a
/// page of the user's.
impl core::fmt::Debug for Response<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Response")
            .field("request", &self.request)
            .field("status", &self.status)
            .field("headers", &self.headers.len())
            .field("body_len", &self.body.len())
            .field("send", &self.send.is_some())
            .finish()
    }
}

impl<'a> Response<'a> {
    /// An answer to the served request `request` (the number its
    /// `event.served_request` carried) with `status` 100-599 and no body.
    ///
    /// # Errors
    /// `Invalid` for a status outside 100-599.
    pub const fn new(request: u32, status: u16) -> Result<Self, Error> {
        if status < 100 || status > 599 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            request,
            status,
            content_type: "",
            headers: &[],
            body: &[],
            send: None,
        })
    }

    /// The `Content-Type` (at most 128 bytes). The host sends one that is not a
    /// media type as `application/octet-stream`; without one it is that too.
    ///
    /// # Errors
    /// `NoSpace` for more than 128 bytes.
    pub const fn content_type(mut self, content_type: &'a str) -> Result<Self, Error> {
        if content_type.len() > CONTENT_TYPE_MAX {
            return Err(Error::NoSpace);
        }
        self.content_type = content_type;
        Ok(self)
    }

    /// Extra headers, at most [`MAX_RESPONSE_HEADERS`]. The host always sends
    /// `Content-Type`, `Content-Length`, `Connection: close` and
    /// `Cache-Control: no-store` itself.
    ///
    /// # Errors
    /// `NoSpace` for more than 8, `Invalid` for a `Content-Type` header (the
    /// host's own; use [`content_type`](Self::content_type)).
    pub fn headers(mut self, headers: &'a [Header<'a>]) -> Result<Self, Error> {
        if headers.len() > MAX_RESPONSE_HEADERS {
            return Err(Error::NoSpace);
        }
        if headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case("content-type"))
        {
            return Err(Error::Invalid);
        }
        self.headers = headers;
        Ok(self)
    }

    /// The body, inline: at most [`MAX_RESPONSE_BODY`] bytes, and not together
    /// with a file to send.
    ///
    /// # Errors
    /// `NoSpace` for a longer body, `Invalid` with a file to send.
    pub fn body(mut self, body: &'a [u8]) -> Result<Self, Error> {
        if body.len() > MAX_RESPONSE_BODY {
            return Err(Error::NoSpace);
        }
        if self.send.is_some() && !body.is_empty() {
            return Err(Error::Invalid);
        }
        self.body = body;
        Ok(self)
    }

    /// A file sent whole as the body, from any root (a plugin serves its
    /// shipped page from [`FileRoot::Install`]). A missing file is answered 404
    /// by the host.
    ///
    /// # Errors
    /// `Invalid` for an empty path or with an inline body, `NoSpace` for a path
    /// over 512 bytes.
    pub fn send_file(mut self, root: FileRoot, path: &'a str) -> Result<Self, Error> {
        check_path(path)?;
        if !self.body.is_empty() {
            return Err(Error::Invalid);
        }
        self.send = Some((root, path));
        Ok(self)
    }
}

impl Net {
    /// Starts `server` (`docs/abi/network.md` 5.1). The outcome is an
    /// `event.server`: started with the address, failed or stopped. Serving
    /// again under a token in use stops the old server first. `input` needs 64
    /// bytes and `out` 1,536.
    ///
    /// # Errors
    /// `State` outside a callback of one of the plugin's own screens that is
    /// showing (a server lives on a screen and stops with it), `Busy` at 2
    /// servers of the plugin or the device's most (1 on the X4), `Unsupported`
    /// for the cable where there is none, `NoSpace` over the caps; otherwise the
    /// host's.
    pub fn serve(
        &self,
        host: &Host<'_>,
        server: &Server,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = Row::Serve(wire::ServiceNetServe {
            token: server.token,
            port: u32::from(server.port),
            flags: 0,
            max_body: server.max_body,
        });
        none(self.call(host, schema::OP_SERVE, core::iter::once(row), input, out)?)
    }

    /// Stops the plugin's server with `token`. A token with no server (never
    /// served, stopped already, ended by the network) is fine and tells nothing:
    /// a plugin may stop on every close.
    ///
    /// # Errors
    /// The host's refusal, or `Invalid` for a reply that is not empty.
    pub fn stop_serving(
        &self,
        host: &Host<'_>,
        token: u32,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        let row = Row::Token(wire::ServiceNetToken { token });
        none(self.call(
            host,
            schema::OP_STOP_SERVING,
            core::iter::once(row),
            input,
            out,
        )?)
    }

    /// Answers one served request (`docs/abi/network.md` 5.3). `input` holds the
    /// encoded response (at most [`REQUEST_INPUT`](super::REQUEST_INPUT) bytes)
    /// and `out` 1,536.
    ///
    /// # Errors
    /// `NotFound` for a request that is answered or ended already, or another
    /// plugin's; `NoMemory` when the pool has no room for the held body; `NoSpace`
    /// for input over the cap; otherwise the host's.
    pub fn respond(
        &self,
        host: &Host<'_>,
        response: &Response<'_>,
        input: &mut [u8],
        out: &mut [u8],
    ) -> Result<(), Error> {
        fn file(file: Option<(FileRoot, &str)>) -> Option<Row<'_>> {
            file.map(|(root, path)| {
                Row::File(wire::ServiceNetFile {
                    role: 0,
                    root: root.wire(),
                    path,
                })
            })
        }
        let head = Row::Response(wire::ServiceNetResponse {
            request: response.request,
            status: u32::from(response.status),
            content_type: response.content_type,
        });
        let rows = core::iter::once(head)
            .chain(response.headers.iter().map(|header| {
                Row::Header(wire::ServiceNetHeader {
                    name: header.name,
                    value: header.value,
                })
            }))
            .chain(file(response.send))
            .chain(
                (!response.body.is_empty()).then_some(Row::Body(wire::ServiceNetBody {
                    data: response.body,
                })),
            );
        none(self.call(host, schema::OP_RESPOND, rows, input, out)?)
    }
}

/// These ops reply with nothing.
fn none(reply_len: usize) -> Result<(), Error> {
    if reply_len != 0 {
        return Err(Error::Invalid);
    }
    Ok(())
}
