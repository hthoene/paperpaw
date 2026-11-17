//! Asking the reader for text, a folder, one of a few options or yes or
//! no, with the answer as a typed event ([`Typed`], [`FolderChosen`],
//! [`Picked`], [`Confirmed`]) the plugin hears with
//! `Items::on`:
//!
//! ```
//! use paperpaw_sdk::ask;
//! use paperpaw_sdk::event::Event;
//! use paperpaw_sdk::replies::Typed;
//!
//! const NAMED: Event<Typed> = Event::new("demo.named");
//!
//! // In an action or a screen:
//! let command = ask::text("Name of the shelf", &NAMED)
//!     .initial("Novels")
//!     .with("shelf", "3")
//!     .command();
//! // r.on(NAMED, |env, typed, out| { typed.text, typed.context.get("shelf") … })
//! # let _ = command;
//! ```
//!
//! The keyboard (slot [`KEYBOARD`](paperpaw_sdk::builtin::slots::KEYBOARD)),
//! the folder chooser ([`CHOOSE_FOLDER`](paperpaw_sdk::builtin::slots::CHOOSE_FOLDER)),
//! the choice ([`CHOICE`](paperpaw_sdk::builtin::slots::CHOICE)) and the yes or
//! no question ([`CONFIRM`](paperpaw_sdk::builtin::slots::CONFIRM)) are screens
//! another plugin can replace: they answer every question asked this way.

use alloc::borrow::Cow;
use alloc::string::String;

use paperpaw_sdk::builtin::{params, slots};
use paperpaw_sdk::Command;
use paperpaw_sdk::event::Event;
use paperpaw_sdk::Params;
use paperpaw_sdk::replies::{Confirmed, FolderChosen, Typed};
use crate::kit::ui::choice::Picked;

/// A question for text (see [`text`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct TextQuestion {
    params: Params,
}

/// Asks for text, under `title` (already translated); the answer comes as
/// `reply`.
pub fn text(title: impl Into<String>, reply: &Event<Typed>) -> TextQuestion {
    TextQuestion {
        params: Params::new()
            .with(params::TITLE, title)
            .with(params::REPLY, reply.as_str()),
    }
}

impl TextQuestion {
    /// The text to start from.
    pub fn initial(mut self, text: impl Into<String>) -> Self {
        self.params.set(params::TEXT, text);
        self
    }

    /// Shown as dots, e.g. a password.
    pub fn password(mut self) -> Self {
        self.params.set(params::MODE, "password");
        self
    }

    /// Digits only.
    pub fn number(mut self) -> Self {
        self.params.set(params::MODE, "number");
        self
    }

    /// Text of several lines (a message, a note): the keyboard has a key
    /// for the line break, and the text comes back with `\n` in it. The
    /// field shows the end of the text on several lines, wrapped.
    pub fn multiline(mut self) -> Self {
        self.params.set(params::MODE, "multiline");
        self
    }

    /// A web address: typed as it is, never with capitals of its own.
    pub fn url(mut self) -> Self {
        self.params.set(params::MODE, "url");
        self
    }

    /// An e-mail address: typed as it is, never with capitals of its own.
    pub fn email(mut self) -> Self {
        self.params.set(params::MODE, "email");
        self
    }

    /// Answers on each change too, with [`Typed::typing`] set, e.g. for a
    /// list that filters as the reader types; the last answer, when the
    /// reader is done, has it unset.
    pub fn live(mut self) -> Self {
        self.params.set(params::LIVE, "1");
        self
    }

    /// Passes `value` along: it comes back in the answer's context.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// Passes all of `context` along (see [`TextQuestion::with`]).
    pub fn context(mut self, context: &Params) -> Self {
        for (key, value) in context.iter() {
            self.params.set(String::from(key), value);
        }
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::KEYBOARD, self.params)
    }
}

/// A question for a folder (see [`folder`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct FolderQuestion {
    params: Params,
}

/// Asks for a folder; the answer comes as `reply`.
pub fn folder(reply: &Event<FolderChosen>) -> FolderQuestion {
    FolderQuestion {
        params: Params::new().with(params::REPLY, reply.as_str()),
    }
}

impl FolderQuestion {
    /// Where to start browsing.
    pub fn start(mut self, path: impl Into<String>) -> Self {
        self.params.set(params::TEXT, path);
        self
    }

    /// Passes `value` along: it comes back in the answer's context.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::CHOOSE_FOLDER, self.params)
    }
}

/// A question with a few options (see [`choice`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct ChoiceQuestion {
    params: Params,
}

/// Asks to choose one of a few options, under `title` (already
/// translated); the option chosen comes as `reply`, nothing when the
/// reader goes back. Asked on the screen of slot
/// [`CHOICE`](paperpaw_sdk::builtin::slots::CHOICE):
///
/// ```
/// use paperpaw_sdk::ask;
/// use paperpaw_sdk::event::Event;
/// use paperpaw_sdk::ui::choice::Picked;
///
/// const CHOSEN: Event<Picked> = Event::new("demo.chosen");
/// let command = ask::choice("Delete “a.epub”?", &CHOSEN)
///     .about("books/a.epub")
///     .options("demo.option.", &["delete", "cancel"])
///     .command();
/// // r.on(CHOSEN, |env, picked, out| { picked.path, picked.option … })
/// # let _ = command;
/// ```
pub fn choice(title: impl Into<String>, reply: &Event<Picked>) -> ChoiceQuestion {
    ChoiceQuestion {
        params: Params::new()
            .with(params::TITLE, title)
            .with(params::REPLY, reply.as_str()),
    }
}

impl ChoiceQuestion {
    /// What it is about, e.g. a file's path: it comes back as
    /// [`Picked::path`].
    pub fn about(mut self, context: impl Into<String>) -> Self {
        self.params.set(params::PATH, context);
        self
    }

    /// The options, each shown as the translation of `labels` followed
    /// by the option (e.g. `my.option.` and `delete`).
    pub fn options(mut self, labels: &str, options: &[&str]) -> Self {
        self.params
            .set(crate::kit::ui::choice::LABELS, String::from(labels));
        self.params
            .set(crate::kit::ui::choice::OPTIONS, options.join(","));
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::CHOICE, self.params)
    }
}

/// A yes or no question (see [`confirm`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use = "a question is asked with its command()"]
pub struct ConfirmQuestion {
    params: Params,
}

/// Asks yes or no, with `question` (already translated) as the title; the
/// answer comes as `reply`, a [`Confirmed`], nothing when the reader goes
/// back or taps outside. The buttons say *Yes* and *No* unless given
/// words of their own; the focus starts on no, the safe answer, unless
/// [`focus_yes`](ConfirmQuestion::focus_yes). Asked on the screen of slot
/// [`CONFIRM`](paperpaw_sdk::builtin::slots::CONFIRM):
///
/// ```
/// use paperpaw_sdk::ask;
/// use paperpaw_sdk::event::Event;
/// use paperpaw_sdk::replies::Confirmed;
///
/// const DELETE: Event<Confirmed> = Event::new("demo.delete");
/// let command = ask::confirm("Delete “a.epub”?", &DELETE)
///     .text("It goes for good.")
///     .yes("Delete")
///     .no("Keep")
///     .with("path", "books/a.epub")
///     .command();
/// // r.on(DELETE, |env, answer, out| if answer.yes { answer.context.get("path") … })
/// # let _ = command;
/// ```
pub fn confirm(question: impl Into<String>, reply: &Event<Confirmed>) -> ConfirmQuestion {
    ConfirmQuestion {
        params: Params::new()
            .with(params::TITLE, question)
            .with(params::REPLY, reply.as_str()),
    }
}

impl ConfirmQuestion {
    /// A line of text below the question, e.g. what the answer does
    /// (already translated; each line break starts a paragraph).
    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.params.set(params::TEXT, text);
        self
    }

    /// The word on the yes button (already translated), e.g. *Delete*.
    pub fn yes(mut self, label: impl Into<String>) -> Self {
        self.params.set(crate::kit::ui::confirm::YES_LABEL, label);
        self
    }

    /// The word on the no button (already translated), e.g. *Keep*.
    pub fn no(mut self, label: impl Into<String>) -> Self {
        self.params.set(crate::kit::ui::confirm::NO_LABEL, label);
        self
    }

    /// One button only, yes (by default *OK*): no question, a text shown
    /// to be read whole, e.g. an explanation a screen has no room for; the
    /// page buttons scroll it where it is long.
    pub fn only_yes(mut self) -> Self {
        self.params.set(crate::kit::ui::confirm::ONLY_YES, "1");
        self
    }

    /// The focus starts on yes, where that is the answer to expect and
    /// does no harm.
    pub fn focus_yes(mut self) -> Self {
        self.params.set(crate::kit::ui::confirm::FOCUS_YES, "1");
        self
    }

    /// Passes `value` along: it comes back in the answer's context.
    pub fn with(mut self, key: impl Into<Cow<'static, str>>, value: impl Into<String>) -> Self {
        self.params.set(key, value);
        self
    }

    /// Passes all of `context` along (see [`ConfirmQuestion::with`]).
    pub fn context(mut self, context: &Params) -> Self {
        for (key, value) in context.iter() {
            self.params.set(String::from(key), value);
        }
        self
    }

    /// The command that asks.
    pub fn command(self) -> Command {
        Command::OpenSlot(slots::CONFIRM, self.params)
    }
}
