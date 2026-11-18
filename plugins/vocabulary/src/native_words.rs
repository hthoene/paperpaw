//! The words of the screens: texts as the catalog writes them. Over [`Words`],
//! so the rows of the screens are made and tested without a host; the host's
//! catalog is behind [`HostWords`].

use alloc::string::String;

use paperpaw_sdk::native::formatting::Arg;
use paperpaw_sdk::native::{Error, Host};
use paperpaw_ui_kit::Messages;

/// Catalog texts of the plugin, made fallibly.
pub(crate) trait Words {
    /// The text of `key`.
    fn copy(&mut self, key: &str) -> Result<String, Error>;
    /// The text of `key` with its names filled in; `Arg::Count` selects the
    /// plural form.
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error>;
}

/// The host's catalog.
pub(crate) struct HostWords<'h, 'a> {
    host: &'h Host<'a>,
    messages: Messages,
}

impl<'h, 'a> HostWords<'h, 'a> {
    /// The catalog of the language the reader chose.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub(crate) fn new(host: &'h Host<'a>) -> Result<Self, Error> {
        Ok(Self {
            host,
            messages: Messages::new()?,
        })
    }
}

impl Words for HostWords<'_, '_> {
    fn copy(&mut self, key: &str) -> Result<String, Error> {
        self.messages.copy(self.host, key)
    }
    fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
        self.messages.format(self.host, key, args)
    }
}

/// The plugin's catalogs for the tests, as the host would resolve them.
#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::sync::tests::heap::fixture;
    use alloc::string::ToString;
    use alloc::vec::Vec;
    use paperpaw_i18n::{Arg as Message, Catalog, Translator};

    /// The plugin's catalog in one language, as the host would resolve it. Its own
    /// allocations are not the plugin's: they are neither counted nor refused.
    pub(crate) struct Catalogued(Translator);

    pub(crate) fn words(language: &str) -> Catalogued {
        let source = match language {
            "de" => include_str!("../i18n/de.ini"),
            _ => include_str!("../i18n/en.ini"),
        };
        let mut translator = Translator::new();
        translator.add(Catalog::parse(source).unwrap());
        translator.set_language(language);
        Catalogued(translator)
    }

    impl Words for Catalogued {
        fn copy(&mut self, key: &str) -> Result<String, Error> {
            Ok(fixture(|| self.0.tr(key).to_string()))
        }
        fn format(&mut self, key: &str, args: &[Arg<'_>]) -> Result<String, Error> {
            Ok(fixture(|| {
                let mut named: Vec<(&str, Message<'_>)> = Vec::new();
                let mut count = None;
                for arg in args {
                    match *arg {
                        Arg::Text { name, value } => named.push((name, Message::Str(value))),
                        Arg::Int { name, value } => named.push((name, Message::Int(value))),
                        Arg::Count(n) => count = Some(n),
                        _ => {}
                    }
                }
                match count {
                    Some(n) => self.0.plural(key, n, &named),
                    None => self.0.format(key, &named),
                }
            }))
        }
    }
}
