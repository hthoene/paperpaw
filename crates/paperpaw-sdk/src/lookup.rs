//! Lookup providers: where a word is looked up, for whatever screen shows
//! it (the standard one is the dictionary's panel in the slot
//! [`slots::LOOKUP`](crate::builtin::slots::LOOKUP)).
//!
//! A provider ([`LookupProviderSpec`], `Items::add_lookup_provider`)
//! answers a [`LookupQuery`] at once with its entries
//! ([`LookupAnswer::Ready`]), or starts work (an HTTP request, say) and
//! answers later ([`LookupAnswer::Pending`]): its plugin then emits the
//! query's reply ([`LookupQuery::answer`], an
//! [`events::LookupAnswered`](crate::builtin::events::LookupAnswered)),
//! keeping the reply's id with its request.
//!
//! The screen in the lookup slot asks the providers
//! ([`LookupProviders::ask`]) and shows every entry with the name of its
//! provider. Those that go online it asks only when the reader chooses
//! to (the shipped panel's *Online*), never with every word. An entry may
//! name a screen that shows more of it ([`LookupEntry::more`], e.g. a
//! whole article), which the reader opens from it.
//!
//! Privacy: that rule is the asking screen's, not the kernel's, and a
//! provider that goes online sends the word where it asks. The answer
//! goes to the asker's reply event (a screen's own id, by default
//! [`events::LOOKUP_ANSWERED`](crate::builtin::events::LOOKUP_ANSWERED)):
//! only plugins that listen for that event hear it, but every open
//! screen sees every event, as with all events.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::{Command, Env, Id};

/// A word to look up.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LookupQuery {
    pub word: String,
    /// The language of the text it is from, e.g. `de`, where known.
    pub language: Option<String>,
    /// The event a provider answering later answers with (see
    /// [`LookupQuery::answer`]).
    pub reply: Id,
}

impl LookupQuery {
    /// `word` in `language`, answered later with
    /// [`events::LOOKUP_ANSWERED`](crate::builtin::events::LOOKUP_ANSWERED).
    pub fn new(word: impl Into<String>, language: Option<String>) -> Self {
        Self {
            word: word.into(),
            language,
            reply: crate::builtin::events::LOOKUP_ANSWERED.id().clone(),
        }
    }

    /// Answered later with `reply` instead, e.g. the asking screen's own
    /// event.
    pub fn reply(mut self, reply: Id) -> Self {
        self.reply = reply;
        self
    }

    /// A later answer of `provider` to the query: its entry from
    /// `source`, none where `text` is empty.
    pub fn answer(
        &self,
        provider: &Id,
        source: impl Into<String>,
        text: impl Into<String>,
    ) -> Command {
        self.answer_entry(provider, LookupEntry::new(source, text))
    }

    /// A later answer of `provider` to the query: `entry`, with the
    /// screen showing more of it where it names one.
    pub fn answer_entry(&self, provider: &Id, entry: LookupEntry) -> Command {
        let answer = crate::builtin::events::LookupAnswered::new(
            provider.clone(),
            self.word.clone(),
            entry.source,
            entry.text,
        )
        .more(entry.more);
        Command::Emit(
            self.reply.clone(),
            crate::event::EventData::to_params(&answer),
        )
    }

    /// Whether `languages` (a provider's) take the query's: any where
    /// either is not known.
    fn fits(&self, languages: &[Cow<'static, str>]) -> bool {
        let Some(language) = &self.language else {
            return true;
        };
        // `de-AT` is German too.
        let base = language.split(['-', '_']).next().unwrap_or(language);
        languages.is_empty()
            || languages
                .iter()
                .any(|l| l.eq_ignore_ascii_case(language) || l.eq_ignore_ascii_case(base))
    }
}

/// What a provider found for a word: a definition, a translation, an
/// article.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LookupEntry {
    /// Where it is from, as shown above it (e.g. a dictionary's name).
    pub source: String,
    /// The text; lines are kept.
    pub text: String,
    /// The screen that shows more of it (a whole article, say), opened
    /// with the word, its language and the entry's text
    /// ([`params::WORD`], [`params::LANGUAGE`], [`params::TEXT`]: what
    /// was fetched need not be fetched again) when the reader asks for
    /// more.
    ///
    /// [`params::WORD`]: crate::builtin::params::WORD
    /// [`params::LANGUAGE`]: crate::builtin::params::LANGUAGE
    /// [`params::TEXT`]: crate::builtin::params::TEXT
    pub more: Option<Id>,
}

impl LookupEntry {
    pub fn new(source: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            text: text.into(),
            more: None,
        }
    }

    /// Shows more of it on `screen` (see [`LookupEntry::more`]).
    pub fn more(mut self, screen: Id) -> Self {
        self.more = Some(screen);
        self
    }
}

/// A provider's answer.
#[derive(Debug)]
#[non_exhaustive]
pub enum LookupAnswer {
    /// Its entries, none where it has none.
    Ready(Vec<LookupEntry>),
    /// It answers later with
    /// [`events::LOOKUP_ANSWERED`](crate::builtin::events::LOOKUP_ANSWERED),
    /// after these commands (e.g. its request) ran.
    Pending(Vec<Command>),
}

/// How a provider looks a word up.
pub type LookupFn = Box<dyn Fn(&Env<'_>, &LookupQuery) -> LookupAnswer>;

/// A lookup provider contribution (see the [module documentation](self)).
#[non_exhaustive]
pub struct LookupProviderSpec {
    pub id: Id,
    /// Translation key of its name.
    pub title: Cow<'static, str>,
    /// The languages of the words it knows (`de`, `en` …); any where
    /// empty.
    pub languages: Vec<Cow<'static, str>>,
    /// It goes online: asked only on request, with a network. The
    /// shipped panel waits 20 seconds for answers that come later: a
    /// request should time out sooner (`HttpRequest::timeout`).
    pub online: bool,
    pub lookup: LookupFn,
}

impl fmt::Debug for LookupProviderSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupProviderSpec")
            .field("id", &self.id)
            .field("online", &self.online)
            .finish_non_exhaustive()
    }
}

impl LookupProviderSpec {
    /// Provider `id`, named `title`, looking up with `lookup`: any
    /// language, offline.
    pub fn new(
        id: Id,
        title: impl Into<Cow<'static, str>>,
        lookup: impl Fn(&Env<'_>, &LookupQuery) -> LookupAnswer + 'static,
    ) -> Self {
        Self {
            id,
            title: title.into(),
            languages: Vec::new(),
            online: false,
            lookup: Box::new(lookup),
        }
    }

    /// Knows words of these languages only.
    pub fn languages(mut self, languages: &[&'static str]) -> Self {
        self.languages = languages.iter().map(|l| Cow::Borrowed(*l)).collect();
        self
    }

    /// Goes online.
    pub fn online(mut self) -> Self {
        self.online = true;
        self
    }
}

/// What asking the providers gave: the entries found at once, the
/// providers still to answer, and the commands that start them.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Asked {
    pub entries: Vec<LookupEntry>,
    pub pending: Vec<Id>,
    pub commands: Vec<Command>,
}

/// The registered lookup providers, in the order of the plugins.
#[derive(Debug, Default)]
pub struct LookupProviders {
    list: Vec<LookupProviderSpec>,
}

impl LookupProviders {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn add(&mut self, provider: LookupProviderSpec) {
        self.list.push(provider);
    }

    #[cfg(any(test, feature = "kernel"))]
    #[cfg_attr(docsrs, doc(cfg(feature = "kernel")))]
    pub fn shrink_to_fit(&mut self) {
        self.list.shrink_to_fit();
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// The provider `id`, e.g. for its name.
    pub fn get(&self, id: &Id) -> Option<&LookupProviderSpec> {
        self.list.iter().find(|p| p.id == *id)
    }

    /// The providers that know the query's language, offline or (with a
    /// network) online ones as `online` says.
    pub fn fitting<'s>(
        &'s self,
        env: &Env<'_>,
        query: &'s LookupQuery,
        online: bool,
    ) -> impl Iterator<Item = &'s LookupProviderSpec> {
        let network = env.caps().network;
        self.list
            .iter()
            .filter(move |p| p.online == online && (!online || network) && query.fits(&p.languages))
    }

    /// Asks the fitting providers, offline or online ones, but `skip`
    /// (e.g. the asking screen's own).
    pub fn ask(&self, env: &Env<'_>, query: &LookupQuery, online: bool, skip: &[Id]) -> Asked {
        let mut asked = Asked::default();
        for provider in self.fitting(env, query, online) {
            if skip.contains(&provider.id) {
                continue;
            }
            match (provider.lookup)(env, query) {
                LookupAnswer::Ready(entries) => asked.entries.extend(entries),
                LookupAnswer::Pending(commands) => {
                    asked.pending.push(provider.id.clone());
                    asked.commands.extend(commands);
                }
            }
        }
        asked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_fit_by_their_base() {
        let query = |l: Option<&str>| LookupQuery::new("Haus", l.map(String::from));
        let de: [Cow<'static, str>; 1] = [Cow::Borrowed("de")];
        assert!(query(Some("de-AT")).fits(&de));
        assert!(query(Some("DE")).fits(&de));
        assert!(!query(Some("en")).fits(&de));
        assert!(query(None).fits(&de));
        assert!(query(Some("en")).fits(&[]));
    }
}
