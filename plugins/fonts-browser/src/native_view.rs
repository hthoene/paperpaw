//! What the two screens show, as data without the host: which rows a font's
//! page offers, and the version of a font that is installed.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::{Version, native::Error};
use paperpaw_ui_kit::mem::push;

use crate::native_catalog::Font;

/// Why there is no list to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trouble {
    /// Nothing: there is a list.
    #[default]
    None,
    /// The first fetch is going on.
    Loading,
    /// No connection, or no answer at all.
    Offline,
    /// The library answered with something else than a catalog.
    Failed,
}

/// What a row of a font's page does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Row {
    /// Download the font.
    Download,
    /// Stop the download going on.
    Stop,
    /// Read with it.
    Read,
    /// Use it for the status lines.
    Status,
    /// Download the newer version.
    Update,
    /// Remove it.
    Remove,
}

/// The rows the page of `font` offers: while its download goes on only
/// *Stop*; where it is not installed only *Download*; else choosing it, an
/// update where a newer version is offered, and removing it.
///
/// # Errors
/// [`Error::NoMemory`].
pub fn rows(font: &Font, installed: Option<Version>, downloading: bool) -> Result<Vec<Row>, Error> {
    let mut rows = Vec::new();
    if downloading {
        push(&mut rows, Row::Stop)?;
    } else if installed.is_none() {
        push(&mut rows, Row::Download)?;
    } else {
        push(&mut rows, Row::Read)?;
        push(&mut rows, Row::Status)?;
        if font.newer(installed) {
            push(&mut rows, Row::Update)?;
        }
        push(&mut rows, Row::Remove)?;
    }
    Ok(rows)
}

/// Whether the list asks for the catalog now: once each time it is opened, and
/// one request at a time (two would share `catalog.json.part`).
#[must_use]
pub const fn should_fetch(asked: bool, in_flight: bool) -> bool {
    !asked && !in_flight
}

/// The fonts that are installed: their plugins' ids and versions.
#[derive(Debug, Default)]
pub struct Installed {
    plugins: Vec<(String, Version)>,
}

impl Installed {
    /// Notes a plugin; ids that are no font's, or versions that do not read,
    /// are left out.
    ///
    /// # Errors
    /// [`Error::NoMemory`].
    pub fn note(&mut self, id: &str, version: &str) -> Result<(), Error> {
        if !id.starts_with("font-") {
            return Ok(());
        }
        let Some(version) = Version::parse(version) else {
            return Ok(());
        };
        push(
            &mut self.plugins,
            (paperpaw_ui_kit::text::copy(id)?, version),
        )
    }

    /// The installed version of the plugin `id`.
    #[must_use]
    pub fn version(&self, id: &str) -> Option<Version> {
        self.plugins.iter().find(|(p, _)| p == id).map(|(_, v)| *v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_catalog::Catalog;

    fn font(version: &str) -> Font {
        let json = alloc::format!(
            r#"{{"format":1,"fonts":[{{"id":"fredoka","name":"Fredoka","version":"{version}","packs":[{{"path":"plugin.ini","url":"a","size":1,"sha1":"0123456789012345678901234567890123456789"}}]}}]}}"#
        );
        let fonts = Catalog::read("https://x/f.json", false, json.as_bytes())
            .unwrap()
            .unwrap();
        fonts.into_iter().next().unwrap()
    }

    #[test]
    fn a_font_that_is_not_installed_is_offered_for_download() {
        let rows = rows(&font("1.0.0"), None, false).unwrap();
        assert_eq!(rows, [Row::Download]);
    }

    #[test]
    fn an_installed_font_is_chosen_updated_or_removed() {
        let here = Some(Version::parse("1.0.0").unwrap());
        assert_eq!(
            rows(&font("1.0.0"), here, false).unwrap(),
            [Row::Read, Row::Status, Row::Remove]
        );
        assert_eq!(
            rows(&font("1.1.0"), here, false).unwrap(),
            [Row::Read, Row::Status, Row::Update, Row::Remove]
        );
    }

    #[test]
    fn the_catalog_is_asked_for_once_and_never_twice_at_a_time() {
        assert!(should_fetch(false, false));
        assert!(!should_fetch(true, false));
        assert!(!should_fetch(false, true));
        assert!(!should_fetch(true, true));
    }

    #[test]
    fn a_download_going_on_is_only_stopped() {
        assert_eq!(rows(&font("1.0.0"), None, true).unwrap(), [Row::Stop]);
    }

    #[test]
    fn only_fonts_are_noted_as_installed() {
        let mut installed = Installed::default();
        installed.note("font-fredoka", "1.2.3").unwrap();
        installed.note("clock", "1.0.0").unwrap();
        installed.note("font-bad", "x").unwrap();
        assert_eq!(installed.version("font-fredoka"), Version::parse("1.2.3"));
        assert_eq!(installed.version("clock"), None);
        assert_eq!(installed.version("font-bad"), None);
    }
}
