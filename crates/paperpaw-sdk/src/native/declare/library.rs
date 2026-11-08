//! Library declarations use the same bounded, latched declaration transaction.
use super::{
    Declarations, DeclareError, filled, menus::menu_identifier, records_wire as wire, within,
};

/// One plugin-owned book field; values are kept by the library service.
#[derive(Clone, Copy, Debug)]
pub struct BookFieldDecl<'a>(wire::BookFieldSpec<'a>);
impl<'a> BookFieldDecl<'a> {
    /// Declare a relative field name and a nonempty catalog title key.
    ///
    /// # Errors
    /// Returns a declaration error for malformed names or overlong/empty fields.
    pub fn new(name: &'a str, title: &'a str) -> Result<Self, DeclareError> {
        menu_identifier(
            name,
            wire::BOOK_FIELD_SPEC_NAME_MAX_BYTES,
            "book field name",
        )?;
        filled(title, "book field title")?;
        within(
            title,
            wire::BOOK_FIELD_SPEC_TITLE_MAX_BYTES,
            "book field title",
        )?;
        Ok(Self(wire::BookFieldSpec {
            name,
            title,
            flags: 0,
        }))
    }
    /// Keep the field in the catalog too, with a 64-byte value cap.
    #[must_use]
    pub const fn in_catalog(mut self) -> Self {
        self.0.flags |= 1;
        self
    }
}
impl<F: FnMut(&[u8]) -> Result<(), i32>> Declarations<F> {
    /// Declare an owned field. The host stamps the plugin prefix on its name.
    ///
    /// # Errors
    /// Returns encoding, allocation or latched sink refusals.
    pub fn book_field(&mut self, field: BookFieldDecl<'_>) -> Result<&mut Self, DeclareError> {
        self.accept_record(Ok(field.0))
    }
    /// Declare a badge callback token. The host owns the `core.book-badge` stage;
    /// this declaration does not subscribe to a plugin-defined stage.
    ///
    /// # Errors
    /// Returns name, encoding, allocation or latched sink refusals.
    pub fn book_badge(&mut self, name: &str, token: u32) -> Result<&mut Self, DeclareError> {
        let record = menu_identifier(
            name,
            wire::BOOK_BADGE_SPEC_NAME_MAX_BYTES,
            "book badge name",
        )
        .map(|()| wire::BookBadgeSpec {
            name,
            token,
            flags: 0,
        });
        self.accept_record(record)
    }
}
