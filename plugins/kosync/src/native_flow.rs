//! The requests the native sync makes to the server, host-free: the address,
//! the headers and the body, as interface 6's `server_request` made them, with
//! every allocation fallible and the credentials wiped when the request is
//! dropped. The plugin hands a [`Call`] to the network service; what a request
//! was for is remembered in [`Pending`] until its answer comes.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::{Error, net::RequestId};
use paperpaw_ui_kit::{
    mem::{push, wipe_text},
    text::{concat, copy},
};

/// What a request was for (its callback token on the network service).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Purpose {
    /// The latest position of a book.
    Pull,
    /// This device's position.
    Push,
    /// *Sign in*: the server checks the user and the key.
    Authorize,
    /// *Create account*.
    Create,
}

impl Purpose {
    /// The request's token on the network service.
    #[must_use]
    pub const fn token(self) -> u32 {
        match self {
            Self::Pull => 1,
            Self::Push => 2,
            Self::Authorize => 3,
            Self::Create => 4,
        }
    }
}

/// A request to the sync server, as the network service takes it.
pub struct Call {
    /// The HTTP method.
    pub method: &'static str,
    url: String,
    headers: Vec<(&'static str, String)>,
    body: Option<String>,
}

impl core::fmt::Debug for Call {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Call")
            .field("method", &self.method)
            .finish_non_exhaustive()
    }
}

impl Call {
    /// A request of `method` to `path` of the server at `base` (a base URL),
    /// as the user `auth.0` with the key `auth.1` if given.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn server(
        base: &str,
        method: &'static str,
        path: &str,
        body: Option<String>,
        auth: Option<(&str, &str)>,
    ) -> Result<Self, Error> {
        let mut headers = Vec::new();
        push(
            &mut headers,
            ("Accept", copy("application/vnd.koreader.v1+json")?),
        )?;
        push(&mut headers, ("Content-Type", copy("application/json")?))?;
        if let Some((user, key)) = auth {
            push(&mut headers, ("x-auth-user", copy(user)?))?;
            push(&mut headers, ("x-auth-key", copy(key)?))?;
        }
        Ok(Self {
            method,
            url: concat(&[base, path])?,
            headers,
            body,
        })
    }

    /// The address.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The headers, name and value.
    pub fn headers(&self) -> impl Iterator<Item = (&'static str, &str)> {
        self.headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
    }

    /// The body, if any.
    #[must_use]
    pub fn body(&self) -> Option<&str> {
        self.body.as_deref()
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        // The key and the password's hash travel in a header and in the body.
        for (_, value) in self.headers.drain(..) {
            wipe_text(value);
        }
        if let Some(body) = self.body.take() {
            wipe_text(body);
        }
    }
}

/// A request out, and what it is waiting for.
#[derive(Debug)]
pub struct Pending {
    /// The service's number of the request.
    pub id: RequestId,
    /// What it is for.
    pub purpose: Purpose,
    /// A fetch: the book. Empty otherwise.
    pub path: String,
    /// A fetch: this device's progress when it was asked (0..1).
    pub local: f64,
    /// A fetch the user asked for.
    pub manual: bool,
}

/// The most requests waiting at once (the service lets a plugin have 4).
pub const MOST_PENDING: usize = 4;

/// The requests out.
#[derive(Debug, Default)]
pub struct Table {
    rows: Vec<Pending>,
}

impl Table {
    /// Whether another request may go out.
    #[must_use]
    pub fn has_room(&self) -> bool {
        self.rows.len() < MOST_PENDING
    }

    /// Notes a request.
    ///
    /// # Errors
    /// `Busy` at the most, `NoMemory`.
    pub fn add(&mut self, pending: Pending) -> Result<(), Error> {
        if !self.has_room() {
            return Err(Error::Busy);
        }
        push(&mut self.rows, pending)
    }

    /// Takes the request the service numbered `id`.
    pub fn take(&mut self, id: RequestId) -> Option<Pending> {
        let at = self.rows.iter().position(|p| p.id == id)?;
        Some(self.rows.swap_remove(at))
    }

    /// Forgets every request (the service ended them).
    pub fn clear(&mut self) {
        self.rows.clear();
    }

    /// How many are out.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether none is out.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// The JSON body of *Create account*: the user and the key.
///
/// # Errors
/// `NoMemory`.
pub fn create_body(user: &str, key: &str) -> Result<String, Error> {
    let user = crate::native_model::json_string(user)?;
    let key = crate::native_model::json_string(key)?;
    let body = concat(&["{\"username\":", &user, ",\"password\":", &key, "}"]);
    wipe_text(key);
    body
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;
    use alloc::format;

    #[test]
    fn a_request_carries_what_interface_6_sent() {
        let call = Call::server(
            "https://sync.example.org",
            "PUT",
            "/syncs/progress",
            Some(copy("{}").unwrap()),
            Some(("anna", "0123")),
        )
        .unwrap();
        assert_eq!(call.url(), "https://sync.example.org/syncs/progress");
        assert_eq!(call.method, "PUT");
        let headers: Vec<_> = call.headers().collect();
        assert_eq!(
            headers,
            [
                ("Accept", "application/vnd.koreader.v1+json"),
                ("Content-Type", "application/json"),
                ("x-auth-user", "anna"),
                ("x-auth-key", "0123"),
            ]
        );
        assert_eq!(call.body(), Some("{}"));
        // Not signed in: no credentials.
        let call = Call::server("https://x", "POST", "/users/create", None, None).unwrap();
        assert_eq!(call.headers().count(), 2);
        assert_eq!(format!("{call:?}"), "Call { method: \"POST\", .. }");
    }

    #[test]
    fn the_account_body_is_json() {
        assert_eq!(
            create_body("an\"na", "0f").unwrap(),
            "{\"username\":\"an\\\"na\",\"password\":\"0f\"}"
        );
    }

    #[test]
    fn at_most_four_requests_wait() {
        let mut table = Table::default();
        for n in 1..=MOST_PENDING as u32 {
            table
                .add(Pending {
                    id: RequestId::new(n).unwrap(),
                    purpose: Purpose::Pull,
                    path: String::new(),
                    local: 0.0,
                    manual: false,
                })
                .unwrap();
        }
        assert!(!table.has_room());
        let again = Pending {
            id: RequestId::new(9).unwrap(),
            purpose: Purpose::Push,
            path: String::new(),
            local: 0.0,
            manual: false,
        };
        assert_eq!(table.add(again), Err(Error::Busy));
        assert!(table.take(RequestId::new(2).unwrap()).is_some());
        assert!(table.take(RequestId::new(2).unwrap()).is_none());
        assert_eq!(table.len(), 3);
    }

    #[test]
    fn every_purpose_has_its_own_token() {
        let tokens = [
            Purpose::Pull,
            Purpose::Push,
            Purpose::Authorize,
            Purpose::Create,
        ]
        .map(Purpose::token);
        for (n, a) in tokens.iter().enumerate() {
            for b in &tokens[n + 1..] {
                assert_ne!(a, b);
            }
        }
    }
}
