//! A plugin's repository checked by the rules of the plugin store at
//! paperpaw.org before it is published, so that a release the site would
//! refuse is never made: `paperpaw-dev check-publish` and
//! `paperpaw-sign check-publish` (which the release workflow of
//! `tools/dev/templates` runs before it releases anything).
//!
//! The rules are the site's own, in one file ([`rules`],
//! `store-rules.json`): the groups, the limits of names, texts, tags and
//! screenshots, what a plugin's folder may hold, the sizes of builds and
//! sources, the readers. The site serves the same file
//! (`/api/v1/store-rules.json`), so a check can take the newest rules, and
//! PaperPaw's CI compares the two.
//!
//! - [`repo::check`]: the repository as it is: `plugin.ini`,
//!   `store/store.ini`, the screenshots, the folder's files, a plugin with
//!   code's `Cargo.lock` and source, the release workflow and its switches.
//! - [`release::check`]: the files a release would carry, as the site reads
//!   them from GitHub: their names (`<id>-<version>.zip`,
//!   `<id>-<version>-<device>.ppn`), each build's chip, plugin interface
//!   and signature, one key for all, the sizes, and the tag.
//!
//! Each [`Problem`] says what is wrong and what to change, in plain words.

pub mod ini;
pub mod kernel;
pub mod release;
pub mod repo;
pub mod rules;
pub mod source;

pub use rules::Rules;

/// How much a problem matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The site would refuse the release.
    Error,
    /// The site takes it, but says so, or it will go wrong later.
    Warning,
}

/// Something to change before publishing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub level: Level,
    /// A short code that stays the same, e.g. `store.group`.
    pub code: &'static str,
    /// Where: a file, perhaps with its line or key.
    pub place: String,
    /// What is wrong and what to change.
    pub message: String,
}

impl Problem {
    pub fn error(code: &'static str, place: impl Into<String>, message: impl Into<String>) -> Self {
        Problem {
            level: Level::Error,
            code,
            place: place.into(),
            message: message.into(),
        }
    }

    pub fn warning(
        code: &'static str,
        place: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Problem {
            level: Level::Warning,
            code,
            place: place.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let level = match self.level {
            Level::Error => "error",
            Level::Warning => "warning",
        };
        write!(
            f,
            "{level} [{}] {}: {}",
            self.code, self.place, self.message
        )
    }
}

/// The plugin interface of the PaperPaw this crate is part of, as the SDK
/// keeps it (`crates/paperpaw-sdk/interface.txt`): what builds for its
/// release are made for.
pub fn interface() -> u16 {
    include_str!("../../../crates/paperpaw-sdk/interface.txt")
        .lines()
        .find_map(|l| l.strip_prefix("interface = "))
        .and_then(|n| n.trim().parse().ok())
        .unwrap_or(0)
}

/// Whether nothing in `problems` stops a release.
pub fn passes(problems: &[Problem]) -> bool {
    problems.iter().all(|p| p.level != Level::Error)
}

/// The arguments both tools take: `[dir] [--release-dir <dir>] [--tag
/// <tag>] [--rules <file>] [--interface <n>]`, and the problems found, the
/// errors first; or what is wrong with the arguments. Builds are checked
/// for `interface`, else this PaperPaw's ([`interface`]).
pub fn run(args: &[String], interface: Option<u16>) -> Result<Vec<Problem>, String> {
    let mut dir = None;
    let (mut release_dir, mut tag, mut rules_file) = (None, None, None);
    let mut interface = interface.or(Some(self::interface()));
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| {
            args.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "--release-dir" => release_dir = Some(value("--release-dir")?),
            "--tag" => tag = Some(value("--tag")?),
            "--rules" => rules_file = Some(value("--rules")?),
            "--interface" => {
                let text = value("--interface")?;
                interface = Some(
                    text.parse()
                        .map_err(|_| format!("--interface takes a number, not {text}"))?,
                );
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            other if dir.is_none() => dir = Some(other.to_string()),
            other => return Err(format!("one repository at a time, not also {other}")),
        }
    }
    let mut problems = Vec::new();
    let rules = match rules_file {
        Some(file) => {
            let text = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
            let rules = Rules::parse(&text).map_err(|e| format!("{file}: {e}"))?;
            if rules != Rules::built_in() {
                problems.push(Problem::warning(
                    "rules.newer",
                    file,
                    "the site's rules differ from the ones this tool was built with: checked \
                     by the site's; update paperpaw-dev or paperpaw-sign to the newest release",
                ));
            }
            rules
        }
        None => Rules::built_in(),
    };
    let dir = std::path::PathBuf::from(dir.unwrap_or_else(|| ".".into()));
    let found = repo::check(&dir, &rules);
    problems.extend(found.problems);
    if let (Some(release_dir), Some(plugin)) = (release_dir, &found.plugin) {
        problems.extend(release::check(
            std::path::Path::new(&release_dir),
            plugin,
            tag.as_deref(),
            interface,
            &rules,
        ));
    } else if let (Some(tag), Some(plugin)) = (tag.as_deref(), &found.plugin) {
        problems.extend(release::check_tag(tag, &plugin.version));
    }
    problems.sort_by_key(|p| p.level);
    Ok(problems)
}
