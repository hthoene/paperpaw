//! *Discover fonts* (the reading settings' entry below the font size): what it
//! does for the fonts browser, as the interface-6 store decided it. Pure
//! functions of what the plugin read of the registry; the plugin carries the
//! step out (`native_plugin.rs`).
//!
//! The fonts browser's list where it runs; otherwise the reader is asked, once,
//! what to do about it (it is not installed, it did not start, it is switched
//! off, or it waits for a restart) and the answer does that one thing. The
//! store's page of it is not the answer to a wish for fonts, except where the
//! plugin is installed and switched on but not running: its page offers what can
//! be done there.
use alloc::string::String;
use paperpaw_sdk::native::{Error, ui::Question};

use crate::native_ask::Say;
use crate::native_installs::Row;
use crate::native_page::Trouble;

/// The fonts browser's id.
pub const PLUGIN: &str = "fonts-browser";
/// The plugin's page, titled by this text while it is fetched.
pub const NAME_KEY: &str = "store.fonts.name";

/// What a yes to the question does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Then {
    /// Open the plugin's page in the store.
    Page,
    /// Switch the plugin on and restart.
    On,
    /// Restart.
    Restart,
}

/// What *Discover fonts* does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Run the fonts browser's action.
    Run,
    /// Open the plugin's page.
    Page,
    /// Ask the reader (see [`Words`]).
    Ask(Words),
}

/// The texts of a question, by the keys of the store's catalog, and what a yes
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub title: &'static str,
    /// The text under the title; `None` for a plugin that failed (its reason).
    pub help: Option<&'static str>,
    pub yes: &'static str,
    pub no: &'static str,
    pub then: Then,
    /// Whether the text is the reason the plugin did not start.
    pub failed: Option<Trouble>,
}

const MISSING: Words = Words {
    title: "store.fonts.missing",
    help: Some("store.fonts.missing.help"),
    yes: "store.fonts.show",
    no: "store.fonts.not-now",
    then: Then::Page,
    failed: None,
};
const OFF: Words = Words {
    title: "store.fonts.off",
    help: Some("store.fonts.off.help"),
    yes: "store.fonts.switch-on",
    no: "store.fonts.not-now",
    then: Then::On,
    failed: None,
};
const RESTART: Words = Words {
    title: "store.fonts.restart",
    help: Some("store.fonts.restart.help"),
    yes: "store.restart",
    no: "store.fonts.later",
    then: Then::Restart,
    failed: None,
};

/// What to do, given whether the fonts browser's action is registered and what
/// the device has of the plugin.
#[must_use]
pub fn decide(has_action: bool, row: Option<&Row>) -> Step {
    if has_action {
        return Step::Run;
    }
    let Some(row) = row else {
        return Step::Ask(MISSING);
    };
    if let Some(trouble) = row.trouble {
        return Step::Ask(Words {
            title: "store.fonts.failed",
            help: None,
            yes: "store.fonts.show",
            no: "store.fonts.not-now",
            then: Then::Page,
            failed: Some(trouble),
        });
    }
    if !row.enabled {
        Step::Ask(OFF)
    } else if row.restart_needed {
        Step::Ask(RESTART)
    } else {
        Step::Page
    }
}

/// The key of the plugin manager's word for why a plugin did not start.
const fn reason_key(trouble: Trouble) -> &'static str {
    match trouble {
        Trouble::OtherBuild => "plugin-manager.code.other-build",
        _ => "plugin-manager.failed",
    }
}

/// The text under a question that has the reason: the plugin manager's word
/// (or the store's own where the catalog has none) and where to read more.
///
/// # Errors
/// `NoMemory`, or what the catalog refuses.
pub fn reason(say: &mut dyn Say, trouble: Trouble) -> Result<String, Error> {
    let reason = match say.lookup(reason_key(trouble))? {
        Some(text) => text,
        None => say.text("store.fonts.failed.reason")?,
    };
    let help = say.text("store.fonts.failed.help")?;
    let mut out = String::new();
    out.try_reserve_exact(reason.len() + 1 + help.len())
        .map_err(|_| Error::NoMemory)?;
    out.push_str(&reason);
    out.push('\n');
    out.push_str(&help);
    Ok(out)
}

/// The confirmation to show, its texts said into `texts`: title, text, then the
/// two words (`yes\nno`). Call [`Texts::question`] to borrow it.
///
/// # Errors
/// `NoMemory`, or what the catalog refuses.
pub fn texts(say: &mut dyn Say, words: &Words) -> Result<Texts, Error> {
    let title = say.text(words.title)?;
    let text = match (words.help, words.failed) {
        (Some(key), _) => say.text(key)?,
        (None, Some(trouble)) => reason(say, trouble)?,
        (None, None) => String::new(),
    };
    let yes = say.text(words.yes)?;
    let no = say.text(words.no)?;
    let mut labels = String::new();
    labels
        .try_reserve_exact(yes.len() + 1 + no.len())
        .map_err(|_| Error::NoMemory)?;
    labels.push_str(&yes);
    labels.push('\n');
    labels.push_str(&no);
    Ok(Texts {
        title,
        text,
        labels,
    })
}

/// The texts of one question.
#[derive(Debug)]
pub struct Texts {
    title: String,
    text: String,
    labels: String,
}

impl Texts {
    /// The question: a confirmation with the yes focused.
    ///
    /// # Errors
    /// `NoSpace` above a field's cap.
    pub fn question(&self) -> Result<Question<'_>, Error> {
        Question::confirm(&self.title, &self.text)?
            .confirm_labels(&self.labels)?
            .focus_yes()
    }

    /// The title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The text under the title.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The words for yes and no, as `yes\nno`.
    #[must_use]
    pub fn labels(&self) -> &str {
        &self.labels
    }
}
