//! The thirteen hyphenation data plugins: what each holds.
//!
//! The pattern sources stay in the interface-6 plugins (`plugins/<id>/patterns/`)
//! until those are removed; a package takes its patterns, catalogs and
//! texts from there and compiles the table (`paperpaw_hyphen::compile`).

/// The version every package starts at. Devices update a plugin only to a
/// newer version than the one installed, and the code plugins these replace
/// are at 1.0.3 (the English one at 1.1.1).
pub const VERSION: &str = "2.0.0";

/// The data formats version of the `[hyphenation]` file kind (contract:
/// `docs/abi/proposals/declarative-registration.md`).
pub const FORMATS: u32 = 5;

/// One hyphenation data plugin.
#[derive(Debug, Clone, Copy)]
pub struct Package {
    /// The plugin id and the folder's name; also the source plugin in `plugins/`.
    pub id: &'static str,
    /// The language tags it provides, and the table (a file stem) each names.
    pub languages: &'static [(&'static str, &'static str)],
    /// The pattern file in `plugins/<id>/patterns/`.
    pub patterns: &'static str,
    /// The exceptions file there, where it has one.
    pub exceptions: Option<&'static str>,
    /// Characters before and after a hyphen the patterns were made for.
    pub minimums: (u8, u8),
}

/// All of them: the base (English) and twelve languages.
pub const PACKAGES: [Package; 13] = [
    Package {
        id: "hyphenation",
        languages: &[("en", "en")],
        patterns: "hyph-en-us.pat.txt",
        exceptions: Some("hyph-en-us.hyp.txt"),
        minimums: (2, 3),
    },
    Package {
        id: "hyphenation-da",
        languages: &[("da", "da")],
        patterns: "hyph-da.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-de",
        languages: &[("de", "de")],
        patterns: "hyph-de-1996.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-es",
        languages: &[("es", "es")],
        patterns: "hyph-es.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-fr",
        languages: &[("fr", "fr")],
        patterns: "hyph-fr.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-it",
        languages: &[("it", "it")],
        patterns: "hyph-it.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    // Bokmål and the general Norwegian tag share one table.
    Package {
        id: "hyphenation-nb",
        languages: &[("nb", "nb"), ("no", "nb")],
        patterns: "hyph-nb.pat.txt",
        exceptions: Some("hyph-nb.hyp.txt"),
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-nl",
        languages: &[("nl", "nl")],
        patterns: "hyph-nl.pat.txt",
        exceptions: Some("hyph-nl.hyp.txt"),
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-pl",
        languages: &[("pl", "pl")],
        patterns: "hyph-pl.pat.txt",
        exceptions: Some("hyph-pl.hyp.txt"),
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-pt",
        languages: &[("pt", "pt")],
        patterns: "hyph-pt.pat.txt",
        exceptions: Some("hyph-pt.hyp.txt"),
        minimums: (2, 3),
    },
    Package {
        id: "hyphenation-ru",
        languages: &[("ru", "ru")],
        patterns: "hyph-ru.pat.txt",
        exceptions: Some("hyph-ru.hyp.txt"),
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-sv",
        languages: &[("sv", "sv")],
        patterns: "hyph-sv.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
    Package {
        id: "hyphenation-uk",
        languages: &[("uk", "uk")],
        patterns: "hyph-uk.pat.txt",
        exceptions: None,
        minimums: (2, 2),
    },
];

impl Package {
    /// The distinct table stems, in order of first use.
    pub fn tables(&self) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        for &(_, stem) in self.languages {
            if !out.contains(&stem) {
                out.push(stem);
            }
        }
        out
    }
}
