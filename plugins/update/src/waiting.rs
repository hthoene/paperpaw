//! An update that waits for plugins: the update server named a newer
//! PaperPaw, but code plugins in use (installed, switched on, of the
//! version asked about) have no build for it yet (`waiting`), or plugins
//! built in now are not built for it yet (`missing … not-ready`). A search
//! in the background does not offer it; it is kept, shown on the update
//! page, and asked for again once a day at a cheap moment, until it no
//! longer waits. If the reader does not open the update page within a
//! week, the page opens once, at a start (on the X4 every wake), and never
//! again for that version.
//!
//! `waiting.txt` keeps it across starts, small, so that a start reads no
//! more than it (version 1): the version, then `since <unix seconds>`
//! (when it began to wait, `-` while the clock is not known), `checked
//! <unix seconds>` (the last question asked, answered or not), lines
//! `asked` (asked once while the clock was not known), `seen` (the reader
//! looked) and `told` (the page opened by itself), and what it waits for:
//! `plugin <id> <version>` and `crate <name>` lines.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::time::Duration;

use paperpaw_sdk::firmware::{Firmware, compare_versions};
use paperpaw_sdk::work::Work;
use paperpaw_sdk::{Command, Commands, Env, PluginInfo, PluginKind, Storage};

use crate::online::{self, Offer};
use crate::{CHECK_DUE, PLUGIN, file};

const WAITING: &str = "waiting.txt";
/// The most `waiting.txt` is read with: far more than its lines for every
/// plugin a device holds ([`MAX_ENTRIES`] each).
const MAX_LEN: usize = 32 * 1024;
/// Plugins and crates kept at most, each.
const MAX_ENTRIES: usize = 200;
/// How long an update waits before the page opens by itself.
pub(crate) const WEEK_SECONDS: i64 = 7 * 24 * 3600;
/// How often the server is asked again while an update waits.
const DAY_SECONDS: i64 = 24 * 3600;

/// What an update waits for: code plugins (id, version) and crates built
/// in now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Holding {
    pub plugins: Vec<(String, String)>,
    pub crates: Vec<String>,
}

impl Holding {
    pub(crate) fn is_empty(&self) -> bool {
        self.plugins.is_empty() && self.crates.is_empty()
    }

    /// What of it still holds: plugins installed, switched on and of the
    /// version asked about, crates still built in.
    pub(crate) fn still(&self, env: &Env<'_>, firmware: &Firmware) -> Self {
        self.kept(
            |id, version| in_use(env.plugins().iter(), id, version),
            |name| firmware.plugins.iter().any(|c| c.name == name),
        )
    }

    /// What of it `plugin` and `built_in` say still holds.
    fn kept(&self, plugin: impl Fn(&str, &str) -> bool, built_in: impl Fn(&str) -> bool) -> Self {
        Self {
            plugins: self
                .plugins
                .iter()
                .filter(|(id, version)| plugin(id, version))
                .cloned()
                .collect(),
            crates: self
                .crates
                .iter()
                .filter(|name| built_in(name))
                .cloned()
                .collect(),
        }
    }
}

/// Whether the code plugin `id` is among `plugins` at `version` and
/// switched on.
pub(crate) fn in_use<'a>(
    plugins: impl IntoIterator<Item = &'a PluginInfo>,
    id: &str,
    version: &str,
) -> bool {
    plugins.into_iter().any(|p| {
        p.kind == PluginKind::Code
            && p.enabled
            && p.manifest.id.as_str() == id
            && p.manifest.version.to_string() == version
    })
}

/// What `waiting.txt` keeps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Waiting {
    pub version: String,
    pub since: Option<i64>,
    pub checked: Option<i64>,
    pub asked: bool,
    pub seen: bool,
    pub told: bool,
    pub holding: Holding,
}

impl Waiting {
    fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines();
        let version = lines.next()?.trim().to_string();
        if !online::is_version(&version) {
            return None;
        }
        let mut waiting = Self {
            version,
            ..Self::default()
        };
        let seconds = |rest: &str| rest.trim().parse::<i64>().ok();
        for line in lines {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("since ") {
                waiting.since = seconds(rest);
            } else if let Some(rest) = line.strip_prefix("checked ") {
                waiting.checked = seconds(rest);
            } else if let Some(rest) = line.strip_prefix("plugin ") {
                if let Some((id, version)) = rest.split_once(' ') {
                    let entry = (id.to_string(), version.trim().to_string());
                    waiting.holding.plugins.push(entry);
                }
            } else if let Some(rest) = line.strip_prefix("crate ") {
                waiting.holding.crates.push(rest.trim().to_string());
            } else {
                match line {
                    "asked" => waiting.asked = true,
                    "seen" => waiting.seen = true,
                    "told" => waiting.told = true,
                    _ => {}
                }
            }
        }
        Some(waiting)
    }

    fn to_text(&self) -> String {
        let seconds = |s: Option<i64>| s.map_or(String::from("-"), |s| s.to_string());
        let mut text = format!(
            "{}\nsince {}\nchecked {}\n",
            self.version,
            seconds(self.since),
            seconds(self.checked)
        );
        for (flag, set) in [
            ("asked", self.asked),
            ("seen", self.seen),
            ("told", self.told),
        ] {
            if set {
                text.push_str(flag);
                text.push('\n');
            }
        }
        for (id, version) in self.holding.plugins.iter().take(MAX_ENTRIES) {
            text.push_str(&format!("plugin {id} {version}\n"));
        }
        for name in self.holding.crates.iter().take(MAX_ENTRIES) {
            text.push_str(&format!("crate {name}\n"));
        }
        text
    }

    /// A start at `now` (unix seconds, if known): times in the future (the
    /// clock set back) count from now. Whether to ask again (a day since
    /// the last question; while the clock is not known, once) and whether
    /// the page opens by itself (a week waited unseen, not before).
    fn start(&mut self, now: Option<i64>) -> (bool, bool) {
        if let Some(now) = now {
            if self.since.is_none_or(|since| since > now) {
                self.since = Some(now);
            }
            if self.checked.is_some_and(|checked| checked > now) {
                self.checked = None;
            }
        }
        let ask = match now {
            Some(now) => self
                .checked
                .is_none_or(|checked| now - checked >= DAY_SECONDS),
            None => !self.asked,
        };
        let open = !self.seen
            && !self.told
            && matches!((self.since, now), (Some(since), Some(now)) if now - since >= WEEK_SECONDS);
        self.told |= open;
        (ask, open)
    }

    /// A question asked at `now`: counted, answered or not.
    fn tried(&mut self, now: Option<i64>) {
        match now {
            Some(now) => self.checked = Some(now),
            None => self.asked = true,
        }
    }
}

pub(crate) fn read(storage: &dyn Storage) -> Option<Waiting> {
    let text = paperpaw_sdk::read_file(storage, &file(WAITING), MAX_LEN).ok()?;
    Waiting::parse(&String::from_utf8(text).ok()?)
}

fn write(storage: &dyn Storage, waiting: &Waiting) {
    let _ = storage.create_dir(&Env::plugin_dir(PLUGIN.as_str()));
    let _ = storage.write(&file(WAITING), waiting.to_text().as_bytes());
}

/// Nothing waits (any more).
pub(crate) fn clear(storage: &dyn Storage) {
    let _ = storage.remove(&file(WAITING));
}

/// An answer naming `version`, which waits for `holding`, at `now` (unix
/// seconds, if known): kept, a newer version waiting anew.
pub(crate) fn answered(storage: &dyn Storage, version: &str, holding: Holding, now: Option<i64>) {
    let mut waiting = match read(storage) {
        Some(w) if w.version == version => w,
        _ => Waiting {
            version: version.to_string(),
            ..Waiting::default()
        },
    };
    waiting.since = waiting.since.or(now);
    waiting.tried(now);
    waiting.holding = holding;
    write(storage, &waiting);
}

/// The server asked at `now`, while an update waits: the next question
/// not before a day (or, without a clock, not before it is known).
pub(crate) fn tried(storage: &dyn Storage, now: Option<i64>) {
    if let Some(mut waiting) = read(storage) {
        waiting.tried(now);
        write(storage, &waiting);
    }
}

/// The reader looked at the update page.
pub(crate) fn seen(storage: &dyn Storage) {
    if let Some(mut waiting) = read(storage)
        && !waiting.seen
    {
        waiting.seen = true;
        write(storage, &waiting);
    }
}

/// The question again at a cheap moment within `within`, not before
/// `after`.
fn ask(after: Option<Duration>, within: Duration) -> Command {
    let mut work = Work::when_cheap(CHECK_DUE.id().clone(), within);
    if let Some(after) = after {
        work = work.after(after);
    }
    Command::work(work)
}

/// After an answer that waits: asked again in a day, at a cheap moment.
pub(crate) fn later(out: &mut Commands) {
    let day = Duration::from_secs(DAY_SECONDS as u64);
    out.push(ask(Some(day), day * 2));
}

/// At a start: whether the update page opens by itself, and the question
/// asked again where it is due. What waits is kept small in
/// `waiting.txt`; an update kept from before it was written (or with it
/// lost) is looked at once and kept so. Nothing waits any more where the
/// update is gone or what it waits for is no longer in use.
pub(crate) fn at_start(env: &Env<'_>, firmware: &Firmware, out: &mut Commands) -> bool {
    let storage = env.storage();
    if !online::has_offer(storage) {
        clear(storage);
        return false;
    }
    let (mut waiting, made) = match read(storage) {
        Some(waiting) => (waiting, false),
        None => {
            let Some(offer) = online::offer(storage, firmware) else {
                return false;
            };
            let holding = offer.holding(env, firmware);
            if holding.is_empty() {
                return false;
            }
            let waiting = Waiting {
                version: offer.version,
                holding,
                ..Waiting::default()
            };
            (waiting, true)
        }
    };
    // Installed meanwhile (from the card) or no longer newer: nothing
    // waits, nor is the answer kept; nor does it wait where what it waits
    // for is no longer in use.
    if !compare_versions(&waiting.version, &firmware.version).is_gt() {
        clear(storage);
        online::clear_offer(storage);
        return false;
    }
    let holding = waiting.holding.still(env, firmware);
    if holding.is_empty() {
        clear(storage);
        return false;
    }
    let before = waiting.clone();
    waiting.holding = holding;
    let (due, open) = waiting.start(paperpaw_sdk::time::unix_seconds(env));
    if due {
        out.push(ask(None, Duration::from_secs(DAY_SECONDS as u64)));
    }
    if made || waiting != before {
        write(storage, &waiting);
    }
    open
}

/// What `offer` waits for, as far as it holds here.
pub(crate) fn of_offer(offer: &Offer, env: &Env<'_>, firmware: &Firmware) -> Holding {
    let named = Holding {
        plugins: offer
            .waiting
            .lines()
            .filter_map(|line| {
                let (id, version) = line.trim().split_once(' ')?;
                Some((id.to_string(), version.trim().to_string()))
            })
            .collect(),
        crates: offer
            .missing
            .lines()
            .filter_map(|line| {
                let (name, why) = line.split_once(' ')?;
                (why.trim() == "not-ready").then(|| name.to_string())
            })
            .collect(),
    };
    named.still(env, firmware)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_version_1_wrote_is_read() {
        let text = include_str!("../testdata/waiting.txt.v1");
        let waiting = Waiting::parse(text).unwrap();
        assert_eq!(
            waiting,
            Waiting {
                version: String::from("0.5.0"),
                since: Some(1_760_000_000),
                checked: Some(1_760_086_400),
                asked: false,
                seen: false,
                told: true,
                holding: Holding {
                    plugins: alloc::vec![(String::from("dict-xl"), String::from("0.3.1"))],
                    crates: alloc::vec![String::from("chess-for-paperpaw")],
                },
            }
        );
        assert_eq!(Waiting::parse(&waiting.to_text()), Some(waiting));
        assert_eq!(
            Waiting::parse("0.5.0\nsince -\nchecked -\n").map(|w| w.since),
            Some(None)
        );
        assert_eq!(Waiting::parse("soon\n"), None);
    }

    fn info(
        id: &'static str,
        version: (u16, u16, u16),
        kind: PluginKind,
        enabled: bool,
    ) -> PluginInfo {
        PluginInfo::new(
            paperpaw_sdk::Manifest::new(
                paperpaw_sdk::Id::parse(id).unwrap(),
                id,
                paperpaw_sdk::Version::new(version.0, version.1, version.2),
            ),
            paperpaw_sdk::PluginState::Running,
            enabled,
            Default::default(),
            kind,
            None,
            None,
        )
    }

    #[test]
    fn only_code_plugins_in_use_at_that_version_hold() {
        let plugins = [
            info("dict", (0, 3, 1), PluginKind::Code, true),
            info("chess", (1, 2, 0), PluginKind::Code, false),
            info("theme", (1, 0, 0), PluginKind::Data, true),
        ];
        assert!(in_use(&plugins, "dict", "0.3.1"));
        assert!(!in_use(&plugins, "dict", "0.3.0"), "updated meanwhile");
        assert!(!in_use(&plugins, "chess", "1.2.0"), "switched off");
        assert!(!in_use(&plugins, "theme", "1.0.0"), "no code");
        assert!(!in_use(&plugins, "gone", "1.0.0"), "removed");
        let holding = Holding {
            plugins: alloc::vec![
                (String::from("dict"), String::from("0.3.1")),
                (String::from("chess"), String::from("1.2.0")),
            ],
            crates: alloc::vec![String::from("kept-crate"), String::from("gone-crate")],
        };
        let kept = holding.kept(|id, v| in_use(&plugins, id, v), |name| name == "kept-crate");
        assert_eq!(
            kept,
            Holding {
                plugins: alloc::vec![(String::from("dict"), String::from("0.3.1"))],
                crates: alloc::vec![String::from("kept-crate")],
            }
        );
        assert!(!kept.is_empty());
        assert!(Holding::default().is_empty());
    }

    #[test]
    fn a_damaged_record_is_none_and_a_large_one_is_read_whole() {
        assert_eq!(Waiting::parse(""), None);
        assert_eq!(Waiting::parse("\u{0}\u{1}garbage\n"), None);
        // Lines it does not know are passed over.
        assert!(Waiting::parse("0.5.0\nsince 12\nwhat is this\n").is_some());
        let many = Waiting {
            version: String::from("0.5.0"),
            since: Some(1),
            checked: Some(2),
            holding: Holding {
                plugins: (0..MAX_ENTRIES)
                    .map(|i| {
                        (
                            format!("plugin-with-a-long-name-{i}"),
                            String::from("10.20.30"),
                        )
                    })
                    .collect(),
                crates: (0..MAX_ENTRIES)
                    .map(|i| format!("paperpaw-plugin-with-a-long-name-{i}"))
                    .collect(),
            },
            ..Waiting::default()
        };
        let text = many.to_text();
        assert!(text.len() < MAX_LEN, "{} bytes", text.len());
        assert_eq!(Waiting::parse(&text), Some(many));
    }

    fn waiting(since: Option<i64>) -> Waiting {
        Waiting {
            version: String::from("0.5.0"),
            since,
            ..Waiting::default()
        }
    }

    #[test]
    fn asked_once_a_day_and_opened_once_after_a_week() {
        let day = DAY_SECONDS;
        let mut w = waiting(Some(1000));
        w.tried(Some(1000));
        assert_eq!(w.start(Some(1000 + day / 2)), (false, false), "asked today");
        assert_eq!(w.start(Some(1000 + day)), (true, false), "a day on");
        w.tried(Some(1000 + day));
        // Every wake of the day after: not again.
        assert!(!w.start(Some(1000 + day + 60)).0);
        assert_eq!(w.start(Some(1000 + 7 * day)), (true, true), "a week");
        assert!(!w.start(Some(1000 + 8 * day)).1, "once");
        // Looked at: never by itself.
        let mut w = waiting(Some(1000));
        w.seen = true;
        assert!(!w.start(Some(1000 + 20 * day)).1);
    }

    #[test]
    fn without_a_clock_it_asks_once_and_counts_from_when_it_is_known() {
        let day = DAY_SECONDS;
        let mut w = waiting(None);
        assert_eq!(w.start(None), (true, false));
        w.tried(None);
        assert_eq!(w.start(None), (false, false), "not at every wake");
        // The clock known: a question (never one with a time yet), the
        // week from now.
        assert_eq!(w.start(Some(50 * day)), (true, false));
        w.tried(Some(50 * day));
        assert!(!w.start(Some(56 * day)).1);
        assert!(w.start(Some(57 * day)).1);
    }

    #[test]
    fn a_clock_set_back_counts_from_now() {
        let day = DAY_SECONDS;
        let mut w = waiting(Some(100 * day));
        w.tried(Some(100 * day));
        // Set back by years: asked again, the week anew.
        assert_eq!(w.start(Some(10 * day)), (true, false));
        assert_eq!(w.since, Some(10 * day));
        w.tried(Some(10 * day));
        assert!(w.start(Some(11 * day)).0);
    }
}
