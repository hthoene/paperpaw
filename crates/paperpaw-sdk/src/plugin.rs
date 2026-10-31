use alloc::borrow::Cow;
use core::fmt;

use crate::contribution::Registrar;
use crate::id::Id;
use crate::version::Version;

/// Static information about a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Manifest {
    /// Plugin id; also the namespace of all its contributions.
    pub id: Id,
    /// Human-readable name in English. Plugins translate it with the key
    /// `<id>.plugin.name`.
    pub name: Cow<'static, str>,
    /// What the plugin does, one English sentence for plugin managers.
    /// Plugins translate it with the key `<id>.plugin.description`.
    pub description: Cow<'static, str>,
    pub version: Version,
    /// What it uses beyond its own files, e.g. the network: shown before
    /// it is installed, and the kernel refuses what is not declared
    /// (ADR 0024, ADR 0025).
    pub uses: Uses,
    /// How it works, at length, by language: `(language, text)`, the text
    /// plain, paragraphs apart by an empty line ([`Manifest::how`]).
    pub how: &'static [(&'static str, &'static str)],
}

impl Manifest {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, name: impl Into<Cow<'static, str>>, version: Version) -> Self {
        Self {
            id,
            name: name.into(),
            version,
            description: Cow::Borrowed(""),
            uses: Uses::default(),
            how: &[],
        }
    }

    pub fn description(mut self, description: impl Into<Cow<'static, str>>) -> Self {
        self.description = description.into();
        self
    }

    /// How it works, at length, in each language it is written in:
    /// shown by the plugin manager as *How it works*. A table in the
    /// program, so it takes no memory until it is read:
    ///
    /// ```
    /// # use paperpaw_sdk::{Id, Manifest, Version};
    /// static HOW: [(&str, &str); 1] = [("en", "What it does.\n\nWhere it is.")];
    /// // In `manifest`, usually `include_str!("../how/en.txt")` and the others:
    /// let manifest = Manifest::new(Id::new_static("demo"), "Demo", Version::new(1, 0, 0)).how(&HOW);
    /// # assert_eq!(manifest.how.len(), 1);
    /// ```
    pub fn how(mut self, how: &'static [(&'static str, &'static str)]) -> Self {
        self.how = how;
        self
    }

    pub fn uses(mut self, uses: Uses) -> Self {
        self.uses = uses;
        self
    }
}

/// What a plugin uses beyond its own files ([`Manifest::uses`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Uses(u8);

impl Uses {
    /// Nothing beyond its own files.
    pub const NOTHING: Uses = Uses(0);
    /// The network: requests, servers, connections, declared network
    /// work. Without it they are refused
    /// ([`Reason::NotDeclared`](crate::work::Reason::NotDeclared)), and
    /// with it the reader can switch the plugin's network off.
    pub const NETWORK: Uses = Uses(1);
    /// Bluetooth ([`bluetooth`](crate::bluetooth)): scanning, links,
    /// pairing, being seen. Without it its commands fail
    /// ([`BtError::NotAllowed`](crate::bluetooth::BtError)).
    pub const BLUETOOTH: Uses = Uses(2);
    /// The developer link over USB ([`developer`](crate::developer)): for
    /// tools the reader installs and allows under *Developer*. Without it
    /// its commands fail ([`DevError::NotDeclared`](crate::developer::DevError)).
    pub const DEVELOPER: Uses = Uses(4);

    /// Whether everything in `other` is declared.
    pub const fn contains(self, other: Uses) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Uses {
        Uses(bits & (Uses::NETWORK.0 | Uses::BLUETOOTH.0 | Uses::DEVELOPER.0))
    }
}

impl core::ops::BitOr for Uses {
    type Output = Uses;

    fn bitor(self, other: Uses) -> Uses {
        Uses(self.0 | other.0)
    }
}

/// A PaperPaw plugin.
///
/// `register` is called once at start-up. It adds the plugin's contributions;
/// the kernel decides afterwards which ones are active (user configuration
/// and priorities may override them).
pub trait Plugin {
    fn manifest(&self) -> Manifest;

    /// Adds the plugin's contributions with `registrar` (see
    /// [`contribution`](crate::contribution)).
    fn register(&self, registrar: &mut Registrar) -> Result<(), RegisterError>;
}

/// Priority of a default binding. Higher wins; an explicit user setting
/// always wins over any priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(pub i16);

impl Priority {
    /// Used only when nothing else is available (diagnostics, placeholders).
    pub const FALLBACK: Priority = Priority(-100);
    /// The normal priority of first-party defaults.
    pub const DEFAULT: Priority = Priority(0);
    /// For plugins whose purpose is to replace a default.
    pub const PREFERRED: Priority = Priority(100);
}

/// A named UI role (home, library, reader, …) bound to a screen.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SlotSpec {
    pub id: Id,
    /// Translation key of the slot's display name.
    pub title: Cow<'static, str>,
}

impl SlotSpec {
    /// Made of what it needs; the rest as set with its methods.
    pub fn new(id: Id, title: impl Into<Cow<'static, str>>) -> Self {
        Self {
            id,
            title: title.into(),
        }
    }
}

/// Why a contribution was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegisterError {
    /// The id is not in the plugin's namespace.
    Namespace { id: Id },
    /// An id was registered twice.
    Duplicate { id: Id },
    /// A data contribution could not be parsed.
    Parse {
        line: u32,
        message: Cow<'static, str>,
    },
    /// Something else is wrong with the contribution.
    Invalid { message: Cow<'static, str> },
    /// A cause this build has no variant for, by its code (0: not said).
    /// Codes are listed here as they are given and never reused; a build
    /// that doesn't know one treats it as the most general failure of its
    /// kind.
    Other(u16),
}

impl fmt::Display for RegisterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RegisterError::Namespace { id } => {
                write!(f, "`{id}` is outside the plugin's namespace")
            }
            RegisterError::Duplicate { id } => write!(f, "`{id}` is already registered"),
            RegisterError::Parse { line, message } => write!(f, "line {line}: {message}"),
            RegisterError::Invalid { message } => f.write_str(message),
            RegisterError::Other(code) => write!(f, "registration error {code}"),
        }
    }
}

impl core::error::Error for RegisterError {}
