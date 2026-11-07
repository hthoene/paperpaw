//! What the page shows, as rows: the facts the host reported and the words,
//! with no host handle, so the rows can be tested without a device.
use alloc::{string::String, vec::Vec};
use paperpaw_sdk::native::Error;
use paperpaw_ui_kit::{
    Item,
    mem::push,
    text::{concat, copy},
};

/// The most bytes of plugin names a row's note collects.
pub(crate) const MOST_NAMES: usize = 1024;
/// The most pages of built-in plugins read (the host pages 32 rows).
pub(crate) const MOST_PAGES: usize = 8;

/// What a row does when chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Version,
    Source,
    BuiltIn,
    Native,
    Licences,
}

/// What the host said about this device and build.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Facts {
    /// The firmware's version, where there is firmware.
    pub version: Option<String>,
    /// How many plugins are built in.
    pub built_in: u32,
    /// The ids of some of them, in the device's order.
    pub names: Vec<String>,
    /// The interface and draft code plugins must be built for, where
    /// this device loads them.
    pub native: Option<(u32, u32)>,
}

/// The words, from the catalogue.
#[derive(Debug, Default)]
pub(crate) struct Texts {
    pub version: String,
    pub summary: String,
    pub source: String,
    pub plugins: String,
    pub native: String,
    pub native_value: String,
    pub native_note: String,
    pub licences: String,
    pub licences_help: String,
}

/// The rows, in the order the page has always had.
pub(crate) fn rows(texts: &Texts, facts: &Facts) -> Result<Vec<(Kind, Item)>, Error> {
    let mut rows = Vec::new();
    push(
        &mut rows,
        (
            Kind::Version,
            Item::noted("PaperPaw", &texts.version, &texts.summary)?,
        ),
    )?;
    push(
        &mut rows,
        (
            Kind::Source,
            Item::noted(
                &texts.source,
                "",
                crate::SOURCE.trim_start_matches("https://"),
            )?,
        ),
    )?;
    if facts.built_in > 0 {
        let count = built_in_count(facts.built_in)?;
        let names = names(&facts.names)?;
        push(
            &mut rows,
            (
                Kind::BuiltIn,
                Item::noted(&texts.plugins, &count, &names)?,
            ),
        )?;
    }
    if facts.native.is_some() {
        push(
            &mut rows,
            (
                Kind::Native,
                Item::noted(&texts.native, &texts.native_value, &texts.native_note)?,
            ),
        )?;
    }
    push(
        &mut rows,
        (
            Kind::Licences,
            Item::noted(&texts.licences, "›", &texts.licences_help)?,
        ),
    )?;
    Ok(rows)
}

/// "12 ›": how many plugins are built in, and that the row opens them.
fn built_in_count(count: u32) -> Result<String, Error> {
    let mut digits = [0; 20];
    let number = paperpaw_ui_kit::text::decimal_u64(u64::from(count), &mut digits);
    concat(&[number, " ›"])
}

/// The plugins' names for a row's note: without the crate prefix, apart by
/// commas, at most [`MOST_NAMES`] bytes.
pub(crate) fn names(ids: &[String]) -> Result<String, Error> {
    let mut used = 0usize;
    let mut text = String::new();
    for id in ids {
        let name = id.strip_prefix("paperpaw-plugin-").unwrap_or(id);
        let add = name.len() + if text.is_empty() { 0 } else { 2 };
        if used + add > MOST_NAMES {
            break;
        }
        text.try_reserve(add).map_err(|_| Error::NoMemory)?;
        if !text.is_empty() {
            text.push_str(", ");
        }
        text.push_str(name);
        used += add;
    }
    Ok(text)
}

/// The version as About shows it: a beta as one, calmly (`0.5.0 Beta 1`).
/// `format` makes the catalogue's sentence from the release and the number.
pub(crate) fn shown_version(
    version: &str,
    format: impl FnOnce(&str, &str) -> Result<String, Error>,
) -> Result<String, Error> {
    match paperpaw_sdk::firmware::beta(version) {
        Some((release, number)) => format(release, number),
        None => copy(version),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::string::ToString;

    fn texts() -> Texts {
        Texts {
            version: "0.6.0".to_string(),
            summary: "summary".to_string(),
            source: "Source code".to_string(),
            plugins: "Plugins built in".to_string(),
            native: "Code plugins".to_string(),
            native_value: "Interface 7, draft 20".to_string(),
            native_note: "note".to_string(),
            licences: "Licences".to_string(),
            licences_help: "help".to_string(),
        }
    }

    fn kinds(rows: &[(Kind, Item)]) -> Vec<Kind> {
        rows.iter().map(|(kind, _)| *kind).collect()
    }

    #[test]
    fn a_device_without_firmware_or_code_plugins_has_three_rows() {
        let rows = rows(&texts(), &Facts::default()).unwrap();
        assert_eq!(kinds(&rows), [Kind::Version, Kind::Source, Kind::Licences]);
    }

    #[test]
    fn built_in_and_native_rows_come_before_the_licences() {
        let facts = Facts {
            version: Some("0.6.0".to_string()),
            built_in: 2,
            names: ["reader".to_string(), "paperpaw-plugin-menu".to_string()].into(),
            native: Some((7, 20)),
        };
        let rows = rows(&texts(), &facts).unwrap();
        assert_eq!(
            kinds(&rows),
            [
                Kind::Version,
                Kind::Source,
                Kind::BuiltIn,
                Kind::Native,
                Kind::Licences
            ]
        );
        let Item::Row(built_in) = &rows[2].1 else {
            panic!("a row")
        };
        assert_eq!(built_in.value, "2 ›");
        assert_eq!(built_in.note, "reader, menu");
        let Item::Row(source) = &rows[1].1 else {
            panic!("a row")
        };
        assert_eq!(source.note, "github.com/hthoene/paperpaw");
    }

    #[test]
    fn names_stop_at_their_most_and_never_cut_one() {
        let ids: Vec<String> = (0..400).map(|n| std::format!("plugin-{n}")).collect();
        let text = names(&ids).unwrap();
        assert!(text.len() <= MOST_NAMES);
        assert!(text.starts_with("plugin-0, plugin-1, "));
        assert!(!text.ends_with(", "));
        let last = text.rsplit(", ").next().unwrap();
        assert!(ids.iter().any(|id| id == last));
    }

    #[test]
    fn a_beta_shows_as_one() {
        let format = |release: &str, number: &str| copy(&std::format!("{release} Beta {number}"));
        assert_eq!(
            shown_version("0.5.0-beta.2", format).unwrap(),
            "0.5.0 Beta 2"
        );
        assert_eq!(shown_version("0.5.0", format).unwrap(), "0.5.0");
        assert_eq!(shown_version("0.5.0-dev", format).unwrap(), "0.5.0-dev");
    }
}
