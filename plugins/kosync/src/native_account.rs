//! The account screen's decisions, host-free: the rows, what a chosen row does,
//! what the server's answer means and what the line below the rows says. It is
//! interface 6's `AccountScreen` (`account.rs`) without the drawing and the
//! commands, with every allocation fallible.
//!
//! The password never leaves this module as typed: only its MD5 is kept, sent
//! and wiped (`mem::wipe_text`) when it is replaced or dropped, as KOReader does.
use alloc::{string::String, vec::Vec};

use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{mem::push, text::copy};

use crate::native_model::{Secret, normalize_server, password_key};

/// A row of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    /// The server's address.
    Server,
    /// The user name.
    User,
    /// The password.
    Password,
    /// *Sign in*.
    SignIn,
    /// *Create account*.
    Create,
    /// *Sign out* (only while signed in).
    SignOut,
}

/// Which text is being typed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Field {
    /// The server address.
    Server,
    /// The user name.
    User,
    /// The password (typed masked).
    Password,
}

/// What a chosen row asks the plugin to do.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Todo {
    /// Nothing (busy, or the screen's rows changed).
    Nothing,
    /// Ask for the text of a field.
    Ask(Field),
    /// Ask the server: `create` false is *Sign in*.
    Call {
        /// *Create account* rather than *Sign in*.
        create: bool,
    },
    /// Sign out: the key and the user go.
    SignOut,
}

/// What the line below the rows says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Message {
    /// What the state is: no network, signed out or signed in.
    State,
    /// The device has no network.
    NoNetwork,
    /// A user name and a password are needed first.
    Missing,
    /// Waiting for the server.
    Connecting,
    /// The server is now this one; the account is gone if there was one.
    ServerSet {
        /// An account was signed out by it.
        signed_out: bool,
    },
    /// The user name is taken.
    Taken,
    /// The server refused or failed, with this HTTP status (0: none came).
    Failed(u32),
}

/// The account screen's state.
#[derive(Default)]
pub struct Account {
    /// The user name typed (or the account's).
    pub user: String,
    /// The MD5 of the password typed, or of the account's.
    key: Option<Secret>,
    /// The account signed in with (the setting); empty when signed out.
    pub signed_in: String,
    /// Waiting for the server.
    pub busy: bool,
    /// The line below the rows.
    pub message: Option<Message>,
    /// The device has a network.
    pub network: bool,
}

impl core::fmt::Debug for Account {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Account")
            .field("user", &self.user)
            .field("signed_in", &self.signed_in)
            .field("busy", &self.busy)
            .finish_non_exhaustive()
    }
}

impl Account {
    /// The state of an account signed in as `user` (empty when signed out).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn new(user: &str, network: bool) -> Result<Self, Error> {
        Ok(Self {
            user: copy(user)?,
            key: None,
            signed_in: copy(user)?,
            busy: false,
            message: Some(Message::State),
            network,
        })
    }

    /// The rows, in order.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn rows(&self) -> Result<Vec<Row>, Error> {
        let mut rows = Vec::new();
        for row in [
            Row::Server,
            Row::User,
            Row::Password,
            Row::SignIn,
            Row::Create,
        ] {
            push(&mut rows, row)?;
        }
        if !self.signed_in.is_empty() {
            push(&mut rows, Row::SignOut)?;
        }
        Ok(rows)
    }

    /// Whether a password has been typed (the row shows dots).
    #[must_use]
    pub const fn has_password(&self) -> bool {
        self.key.is_some()
    }

    /// The key (the MD5 of the password typed), for the request and for keeping.
    #[must_use]
    pub fn key(&self) -> Option<&str> {
        self.key.as_ref().map(Secret::as_str)
    }

    /// A row was chosen.
    pub fn chosen(&mut self, row: Row) -> Todo {
        if self.busy {
            return Todo::Nothing;
        }
        match row {
            Row::Server => Todo::Ask(Field::Server),
            Row::User => Todo::Ask(Field::User),
            Row::Password => Todo::Ask(Field::Password),
            Row::SignIn | Row::Create => {
                if !self.network {
                    self.message = Some(Message::NoNetwork);
                    return Todo::Nothing;
                }
                if self.key.is_none() || self.user.is_empty() {
                    self.message = Some(Message::Missing);
                    return Todo::Nothing;
                }
                self.busy = true;
                self.message = Some(Message::Connecting);
                Todo::Call {
                    create: row == Row::Create,
                }
            }
            Row::SignOut => {
                self.sign_out();
                Todo::SignOut
            }
        }
    }

    /// Signed out: the account is gone from the screen.
    pub fn sign_out(&mut self) {
        self.signed_in.clear();
        self.message = Some(Message::State);
    }

    /// The user name was typed (leading and trailing spaces dropped).
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn typed_user(&mut self, text: &str) -> Result<(), Error> {
        self.user = copy(text.trim())?;
        Ok(())
    }

    /// The password was typed: its MD5 is kept, the password itself is not.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn typed_password(&mut self, text: &str) -> Result<(), Error> {
        self.key = None;
        if !text.is_empty() {
            self.key = Some(password_key(text)?);
        }
        Ok(())
    }

    /// The server address was typed. `current` is the base URL set now; the
    /// result is the new base URL when it differs (the account, if any, is then
    /// signed out) and `None` when it is the same.
    ///
    /// # Errors
    /// `NoMemory`.
    pub fn typed_server(&mut self, text: &str, current: &str) -> Result<Option<String>, Error> {
        let url = normalize_server(text)?;
        if url == current {
            return Ok(None);
        }
        let signed_out = !self.signed_in.is_empty();
        if signed_out {
            self.sign_out();
        }
        self.message = Some(Message::ServerSet { signed_out });
        Ok(Some(url))
    }

    /// The row to focus after a field was typed.
    #[must_use]
    pub const fn next_focus(field: Field) -> usize {
        match field {
            Field::Server => 1,
            Field::User => 2,
            Field::Password => 3,
        }
    }

    /// The server's answer to *Sign in* (`created` false) or *Create account*
    /// with this HTTP `status` (0: none came). Success keeps the user as signed
    /// in; the plugin keeps the key and the setting. Returns whether it did.
    pub fn answered(&mut self, created: bool, status: u32) -> Result<bool, Error> {
        self.busy = false;
        let ok = if created { 201 } else { 200 };
        if status == ok {
            self.signed_in = copy(&self.user)?;
            self.message = Some(Message::State);
            return Ok(true);
        }
        self.message = Some(if created && status == 402 {
            Message::Taken
        } else {
            Message::Failed(status)
        });
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, reason = "fixtures")]
    use super::*;

    fn account(user: &str) -> Account {
        Account::new(user, true).unwrap()
    }

    #[test]
    fn the_rows_follow_whether_signed_in() {
        let rows = account("").rows().unwrap();
        assert_eq!(
            rows,
            [
                Row::Server,
                Row::User,
                Row::Password,
                Row::SignIn,
                Row::Create
            ]
        );
        assert_eq!(account("anna").rows().unwrap().last(), Some(&Row::SignOut));
    }

    #[test]
    fn nothing_goes_out_without_user_and_password() {
        let mut a = account("");
        assert_eq!(a.chosen(Row::SignIn), Todo::Nothing);
        assert_eq!(a.message, Some(Message::Missing));
        a.typed_user("  anna ").unwrap();
        assert_eq!(a.user, "anna");
        assert_eq!(a.chosen(Row::Create), Todo::Nothing);
        a.typed_password("secret").unwrap();
        assert_eq!(a.chosen(Row::Create), Todo::Call { create: true });
        assert!(a.busy);
        assert_eq!(a.message, Some(Message::Connecting));
        // Waiting: no second request, no new field.
        assert_eq!(a.chosen(Row::SignIn), Todo::Nothing);
        assert_eq!(a.chosen(Row::User), Todo::Nothing);
    }

    #[test]
    fn without_a_network_nothing_goes_out() {
        let mut a = Account::new("", false).unwrap();
        a.typed_user("anna").unwrap();
        a.typed_password("pw").unwrap();
        assert_eq!(a.chosen(Row::SignIn), Todo::Nothing);
        assert_eq!(a.message, Some(Message::NoNetwork));
        assert!(!a.busy);
    }

    #[test]
    fn only_the_md5_of_the_password_is_kept() {
        let mut a = account("");
        assert!(!a.has_password());
        a.typed_password("abc").unwrap();
        assert_eq!(a.key(), Some("900150983cd24fb0d6963f7d28e17f72"));
        a.typed_password("").unwrap();
        assert!(!a.has_password());
        assert_eq!(a.key(), None);
    }

    #[test]
    fn the_answer_to_the_other_request_is_no_success() {
        for (created, status, ok) in [
            (false, 200, true),
            (false, 201, false),
            (true, 201, true),
            (true, 200, false),
            (false, 401, false),
            (false, 0, false),
        ] {
            let mut a = account("");
            a.typed_user("anna").unwrap();
            a.busy = true;
            assert_eq!(
                a.answered(created, status).unwrap(),
                ok,
                "{created} {status}"
            );
            assert!(!a.busy);
            assert_eq!(a.signed_in == "anna", ok);
        }
    }

    #[test]
    fn a_taken_name_and_failures_are_told() {
        let mut a = account("");
        a.answered(true, 402).unwrap();
        assert_eq!(a.message, Some(Message::Taken));
        // 402 means "taken" only for a new account.
        a.answered(false, 402).unwrap();
        assert_eq!(a.message, Some(Message::Failed(402)));
        a.answered(false, 0).unwrap();
        assert_eq!(a.message, Some(Message::Failed(0)));
    }

    #[test]
    fn another_server_signs_out_the_same_one_does_not() {
        let mut a = account("anna");
        assert_eq!(
            a.typed_server("sync.example.org/", "https://sync.example.org")
                .unwrap(),
            None
        );
        assert_eq!(a.signed_in, "anna");
        let url = a
            .typed_server("other.example", "https://sync.example.org")
            .unwrap();
        assert_eq!(url.as_deref(), Some("https://other.example"));
        assert!(a.signed_in.is_empty());
        assert_eq!(a.message, Some(Message::ServerSet { signed_out: true }));
        // Signed out already: only the new server is said.
        a.typed_server("third.example", "https://other.example")
            .unwrap();
        assert_eq!(a.message, Some(Message::ServerSet { signed_out: false }));
    }

    #[test]
    fn signing_out_is_a_row_and_clears_the_account() {
        let mut a = account("anna");
        assert_eq!(a.chosen(Row::SignOut), Todo::SignOut);
        assert!(a.signed_in.is_empty());
        assert_eq!(a.rows().unwrap().len(), 5);
    }

    #[test]
    fn typing_moves_the_focus_on() {
        assert_eq!(Account::next_focus(Field::Server), 1);
        assert_eq!(Account::next_focus(Field::User), 2);
        assert_eq!(Account::next_focus(Field::Password), 3);
    }
}
