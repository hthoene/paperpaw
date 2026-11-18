//! Declared ways in (see [`super`]): for the screens of PaperPaw's own app
//! that the searches do not find within their bounds, the way a reader
//! goes there, written down, replayed and verified on every run.
//!
//! The searches press every button in every place; they do not come by a
//! text typed, a widget placed, a book with a footnote, a theme
//! installed. And the app is too large to search to the end (#351): a
//! screen they do not find is not decided. A way declared here decides
//! it: the steps from the home screen ([`Driver::step`] has what a step
//! can be), on a device as at the start, with what the way needs besides
//! (settings, files in the library). Where the steps end on the screen it
//! is reached (`reach.declared`, a note), and closing and the focus are
//! checked there as on a screen the search found; where they do not, that
//! is an error (`reach.way-lost`), with the step it ended at.
//!
//! A way is written for the buttons of the X4s; on a device that lacks
//! one of them (`two-buttons`) the screen is left to the searches.
//!
//! To write one, try its steps: `PAPERPAW_REACH_TRY=<screen>=<steps>
//! paperpaw-dev reach --quick --network fixtures --profile xteink-x4`
//! (steps with commas between them; `<screen>` alone takes the declared
//! ones) starts the device as the way declared for `<screen>` has it,
//! if one is, and prints what shows after each step.

use paperpaw_emulator::profiles::Profile;

use super::{Driver, Limits};
use crate::report::Finding;
use crate::session::Start;

/// A screen's declared way in.
#[derive(Debug, Clone, Copy)]
pub struct Way {
    pub screen: &'static str,
    /// What the way needs that the searches do not come by, for the
    /// report: it ends a sentence beginning "It needs".
    pub needs: &'static str,
    /// Settings before the app starts, as `(id, value)`.
    pub settings: &'static [(&'static str, &'static str)],
    /// Files put into the library, as `(path, bytes)`.
    pub files: &'static [(&'static str, &'static [u8])],
    /// The steps from the home screen.
    pub steps: &'static [&'static str],
}

impl Way {
    /// How its device starts.
    pub fn start(&self) -> Start {
        Start {
            settings: self
                .settings
                .iter()
                .map(|(id, value)| (id.to_string(), value.to_string()))
                .collect(),
            files: self
                .files
                .iter()
                .map(|(path, bytes)| (path.to_string(), bytes.to_vec()))
                .collect(),
            ..Start::default()
        }
    }

    pub fn steps(&self) -> Vec<String> {
        self.steps.iter().map(|step| step.to_string()).collect()
    }

    /// Whether `profile` has every button the steps press.
    pub fn fits(&self, profile: &Profile) -> bool {
        self.steps.iter().all(|step| {
            let button = match step.split_once(':') {
                Some(("long", button)) => button,
                Some(_) => return true,
                None => step,
            };
            profile.buttons.contains(&button)
        })
    }
}

/// The way declared for `screen`, if one is that `profile` fits.
pub fn declared(screen: &str, profile: &Profile) -> Option<&'static Way> {
    WAYS.iter()
        .find(|way| way.screen == screen && way.fits(profile))
}

/// No hyphenation: a book opens without the question whether to fetch
/// the patterns of its language (which the fixtures' store offers).
const PLAIN: (&str, &str) = ("core.hyphenate", "false");

/// A book with a footnote on its first page (the sample library's have
/// none): `footnote.epub`, made by `footnote.py` beside it.
const FOOTNOTE: (&str, &[u8]) = ("books/a-footnote.epub", include_bytes!("footnote.epub"));

/// A theme to choose: a data plugin with one.
const THEME: &[(&str, &[u8])] = &[
    (
        "plugins/reach-theme/plugin.ini",
        b"[plugin]\nid = reach-theme\nname = Reach\ndescription = A theme to choose.\n\
          version = 1.0.0\nformats = 1\n",
    ),
    (
        "plugins/reach-theme/presets.ini",
        b"[round]\nkind = core.theme\nname = Round\ndescription = Round corners.\n\
          core.ui-corners = core.corners.soft\n",
    ),
];

/// The declared ways in.
pub const WAYS: &[Way] = &[
    Way {
        screen: "reader.toc",
        needs: "a book open, and its menu's Contents",
        settings: &[PLAIN],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "confirm",
            "to:Contents",
            "confirm",
        ],
    },
    Way {
        screen: "reader.annotations",
        needs: "a book open, and its menu's Notes",
        settings: &[PLAIN],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "confirm",
            "to:Notes",
            "confirm",
        ],
    },
    Way {
        screen: "reader.skim-screen",
        needs: "a book open, and Skim through the book under its menu's More",
        settings: &[PLAIN],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "confirm",
            "to:More",
            "confirm",
            "to:Skim",
            "confirm",
        ],
    },
    Way {
        screen: "reader.search-results",
        needs: "a book open, and a text typed into its search",
        settings: &[PLAIN],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "confirm",
            "to:Search",
            "confirm",
            "type:Alice",
            "wait:3000",
        ],
    },
    Way {
        screen: "reader.footnote",
        needs: "a book with a footnote, shown over the page (`core.footnotes`)",
        settings: &[PLAIN, ("core.footnotes", "core.footnotes.popup")],
        files: &[FOOTNOTE],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "confirm",
            "to:More",
            "confirm",
            "to:Links",
            "confirm",
            "confirm",
        ],
    },
    Way {
        screen: "dictionary.popup",
        needs: "a word chosen on a page with text (Select text under the book's menu's More), \
                and Look up in the bar over it, which the reader does not describe as parts",
        settings: &[PLAIN],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "confirm",
            "wait:2000",
            "page-forward",
            "page-forward",
            "confirm",
            "to:More",
            "confirm",
            "to:Select text",
            "confirm",
            "confirm",
            "left",
            "confirm",
        ],
    },
    Way {
        screen: "settings.search",
        needs: "a text typed into the settings' search",
        settings: &[],
        files: &[],
        steps: &[
            "long:confirm",
            "to:Settings",
            "confirm",
            "to:Search",
            "confirm",
            "type:light",
        ],
    },
    Way {
        screen: "settings.themes",
        needs: "a theme installed (a data plugin), which Appearance then offers",
        settings: &[],
        files: THEME,
        steps: &[
            "long:confirm",
            "to:Settings",
            "confirm",
            "to:Appearance",
            "confirm",
            "to:Theme",
            "confirm",
        ],
    },
    Way {
        screen: "store.letters",
        needs: "a group of the store with more plugins than a page holds: the fixtures' \
                store has two in one, more than a page at the largest text and rows",
        settings: &[
            ("core.ui-size", "24"),
            ("core.ui-row", "300"),
            ("core.ui-lines", "2"),
        ],
        files: &[],
        steps: &[
            "long:confirm",
            "to:Tools",
            "confirm",
            "to:Plugin store",
            "confirm",
            "wait:3000",
            "to:Reading",
            "confirm",
            "wait:1000",
            "to:Go to letter",
            "confirm",
        ],
    },
    Way {
        screen: "files.info",
        needs: "a book's sheet in the library (Select held on it), which the library does \
                not describe as a part",
        settings: &[],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "right",
            "long:confirm",
            "to:Book information",
            "confirm",
        ],
    },
    Way {
        screen: "files.shelves",
        needs: "the library's Show row stepped on to the shelves",
        settings: &[],
        files: &[],
        steps: &[
            "to:Start reading",
            "confirm",
            "confirm",
            "confirm",
            "confirm",
            "confirm",
        ],
    },
    Way {
        screen: "fonts-browser.font",
        needs: "a font of the list of fonts to download (the fixtures' catalog has two)",
        settings: &[],
        files: &[],
        steps: &[
            "long:confirm",
            "to:Settings",
            "confirm",
            "to:Reading",
            "confirm",
            "to:Text",
            "confirm",
            "to:Discover fonts",
            "confirm",
            "wait:3000",
            "confirm",
        ],
    },
    Way {
        screen: "widgets.todo.list",
        needs: "the to-do widget placed on the home screen (Edit, +, Notes and pictures, \
                To do), and a press on it",
        settings: &[],
        files: &[],
        steps: &[
            "to:Edit",
            "confirm",
            "to:+",
            "confirm",
            "to:Notes and pictures",
            "confirm",
            "right",
            "right",
            "confirm",
            "to:Done",
            "confirm",
            "to:Nothing to do",
            "confirm",
        ],
    },
];

/// Replays a declared way in on `driver`, a device started with what the
/// way needs: reached where its steps end on its screen (a note, then
/// closing and the focus as on a screen the search found), else an error
/// with the step it ended at and a picture.
pub(super) fn replay(
    driver: &mut Driver<'_>,
    way: &Way,
    home: &str,
    limits: &Limits,
) -> Vec<Finding> {
    let steps = way.steps();
    let seen = driver.go(&steps, None);
    if let Some(seen) = seen.as_ref().filter(|seen| seen.top() == way.screen)
        && driver.lost.is_none()
    {
        let mut findings = vec![Finding::info(
            "reach.declared",
            way.screen.to_string(),
            format!(
                "Reached by its declared way in ({}). It needs {}.",
                steps.join(", "),
                way.needs
            ),
        )];
        findings.extend(super::on_screen(
            driver,
            &steps,
            &seen.state(),
            way.screen,
            home,
            limits,
        ));
        return findings;
    }
    let ended = match (&seen, &driver.lost) {
        (None, _) => String::from("no device could be started for it"),
        (Some(seen), Some(step)) => format!(
            "its step `{step}` could not be done, and the steps end on `{}`",
            seen.top()
        ),
        (Some(seen), None) => format!("the steps end on `{}`", seen.top()),
    };
    let mut lost = Finding::error(
        "reach.way-lost",
        way.screen.to_string(),
        format!(
            "Its declared way in ({}; it needs {}) does not lead to it: {ended}. Put the way \
             right (WAYS in tools/dev/src/reach/ways.rs, which says how to try one), or the \
             screen, if it lost its way in.",
            steps.join(", "),
            way.needs
        ),
    );
    if let Some(path) = super::picture(
        driver.context,
        &driver.variant,
        way.screen,
        "way-lost",
        &driver.session.image(),
    ) {
        lost = lost.picture(path);
    }
    vec![lost]
}

/// Tries steps, for who writes a way (see the module's text): `asked` is
/// `<screen>` or `<screen>=<steps>`. What shows after each step is
/// printed.
pub fn try_out(driver: &mut Driver<'_>, asked: &str) {
    let (screen, steps) = asked.split_once('=').unwrap_or((asked, ""));
    let mut steps: Vec<String> = steps
        .split(',')
        .map(str::trim)
        .filter(|step| !step.is_empty())
        .map(String::from)
        .collect();
    if steps.is_empty()
        && let Some(way) = WAYS.iter().find(|way| way.screen == screen)
    {
        steps = way.steps();
    }
    let show = |driver: &mut Driver<'_>, step: &str| {
        let seen = driver.observe();
        let focus = seen.focused().map(super::name).unwrap_or_default();
        // The parts to act on; where a screen describes none, its texts.
        let mut targets: Vec<String> = seen.targets().map(super::name).collect();
        if targets.is_empty() {
            targets = seen.parts.iter().map(super::name).collect();
        }
        eprintln!(
            "{step}: {} | focus {focus} | {}{}",
            seen.stack.join(" > "),
            targets.join(", "),
            if driver.lost.as_deref() == Some(step) {
                " | NOT DONE"
            } else {
                ""
            }
        );
    };
    if driver.go(&[], None).is_none() {
        return;
    }
    show(driver, "start");
    for step in &steps {
        driver.step(step);
        show(driver, step);
    }
}
