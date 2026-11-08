//! A lookup provider's declaration (`lookup.spec`, `docs/abi/document.md` 7).
use super::menus::menu_identifier;
use super::records_wire as wire;
use super::{Declarations, DeclareError, filled};
use paperpaw_native::host_glue::ERR_INVALID;

/// A provider of the stage `core.lookup`: a name the asker's order setting
/// lists it by, the title the reader sees, the languages it knows and whether
/// it needs the network.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LookupProviderDecl<'a> {
    /// The relative name (`summary`): the id is `<plugin>.<name>`.
    pub name: &'a str,
    /// The title, a catalog key of this plugin.
    pub title: &'a str,
    /// BCP 47 tags, one per line (at most 32), or empty for any language.
    pub languages: &'a str,
    /// Offered only while a network is up.
    pub network: bool,
    /// The callback token the host calls the provider with.
    pub token: u32,
}

impl<'a> LookupProviderDecl<'a> {
    /// A provider for any language that needs no network.
    #[must_use]
    pub const fn new(name: &'a str, title: &'a str, token: u32) -> Self {
        Self {
            name,
            title,
            languages: "",
            network: false,
            token,
        }
    }

    /// Only these languages (tags one per line).
    #[must_use]
    pub const fn languages(mut self, tags: &'a str) -> Self {
        self.languages = tags;
        self
    }

    /// Offered only while a network is up.
    #[must_use]
    pub const fn online(mut self) -> Self {
        self.network = true;
        self
    }
}

/// A BCP 47 tag as the contract spells it: 2 to 35 bytes of ASCII letters and
/// digits in hyphen-separated subtags of 1 to 8.
fn valid_tag(tag: &str) -> bool {
    tag.len() >= 2
        && tag.len() <= wire::LOOKUP_SPEC_LANGUAGES_ITEM_MAX_BYTES
        && tag.split('-').all(|part| {
            (1..=8).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_alphanumeric())
        })
}

impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declares a lookup provider.
    ///
    /// # Errors
    /// Refuses a name that is not a relative id, an empty title, a language
    /// that is not a BCP 47 tag or more than 32 of them. The host admits the
    /// provider when its stage is live.
    pub fn lookup_provider(
        &mut self,
        provider: LookupProviderDecl<'_>,
    ) -> Result<&mut Self, DeclareError> {
        let record = (|| {
            menu_identifier(
                provider.name,
                wire::LOOKUP_SPEC_NAME_MAX_BYTES,
                "lookup name",
            )?;
            filled(provider.title, "lookup title")?;
            if provider.title.len() > wire::LOOKUP_SPEC_TITLE_MAX_BYTES {
                return Err(DeclareError::Encode(ERR_INVALID));
            }
            if !provider.languages.is_empty() {
                let mut count = 0;
                for tag in provider
                    .languages
                    .split(wire::LOOKUP_SPEC_LANGUAGES_SEPARATOR)
                {
                    count += 1;
                    if !valid_tag(tag) || count > wire::LOOKUP_SPEC_LANGUAGES_MAX_ITEMS {
                        return Err(DeclareError::Encode(ERR_INVALID));
                    }
                }
            }
            Ok(wire::LookupSpec {
                name: provider.name,
                title: provider.title,
                languages: provider.languages,
                flags: u32::from(provider.network),
                token: provider.token,
            })
        })();
        self.accept_record(record)
    }
}
