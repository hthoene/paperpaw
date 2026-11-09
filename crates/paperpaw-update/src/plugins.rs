//! The code plugins built into a firmware image ([`Plugins`]).

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

/// Where the list begins in an app image: after the image's header (24
/// bytes), its first segment's (8) and the application descriptor (256).
pub const PLUGINS_AT: usize = 0x120;
/// The list's first bytes.
pub const PLUGINS_MAGIC: [u8; 8] = *b"PPlugins";
/// The list's format this crate reads and writes.
pub const PLUGINS_FORMAT: u16 = 1;
/// The bytes before the text.
pub const PLUGINS_HEAD_LEN: usize = 12;
/// The longest text taken, plenty for hundreds of plugins.
pub const PLUGINS_MAX_LEN: usize = 16 * 1024;

/// A plugin crate built in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PluginCrate<'a> {
    /// The crate's name, e.g. `paperpaw-plugin-stats`.
    pub name: &'a str,
    /// Its version, e.g. `0.4.0`.
    pub version: &'a str,
}

/// What a first-party plugin crate's name begins with: the plugin's id
/// follows.
pub const CRATE_PREFIX: &str = "paperpaw-plugin-";
/// The longest plugin id.
pub const ID_MAX: usize = 64;

impl PluginCrate<'_> {
    /// The plugin id of the crate: `paperpaw-plugin-<id>` is `<id>` where that
    /// is a plugin id (lower-case letters, digits and `-`, at most [`ID_MAX`]
    /// bytes); `None` for any other crate, which neither the op 4 list nor
    /// an update's check compares. Every first-party plugin crate is named
    /// for its folder's id.
    pub fn plugin_id(&self) -> Option<&str> {
        let id = self.name.strip_prefix(CRATE_PREFIX)?;
        let valid = !id.is_empty()
            && id.len() <= ID_MAX
            && id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        valid.then_some(id)
    }
}

/// The code plugins built into a firmware image, so that an update can
/// bring the same ones and the web flasher can choose them again.
///
/// The list sits in the image at [`PLUGINS_AT`], right after the
/// application descriptor (the firmware puts it in the section
/// `.flash.appdesc.plugins`, which the linker places there), so a few
/// bytes read from the start of an app partition or an update file are
/// enough:
///
/// | Bytes | What |
/// |-------|------|
/// | 0..8 | `PPlugins` |
/// | 8..10 | the format, 1 (little-endian) |
/// | 10..12 | the length of the text that follows (little-endian) |
/// | 12.. | the text: a line for each plugin crate, its name and its version, e.g. `paperpaw-plugin-stats 0.4.0` |
///
/// Older images have no list: [`Plugins::read`] then finds nothing.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plugins {
    text: Cow<'static, str>,
}

impl Plugins {
    /// The list from its text, one `name version` a line.
    pub fn new(text: impl Into<Cow<'static, str>>) -> Self {
        Self { text: text.into() }
    }

    /// How long the text is, from the list's first [`PLUGINS_HEAD_LEN`]
    /// bytes; `None` if there is no list (an older image).
    pub fn text_len(head: &[u8]) -> Option<usize> {
        let head = head.get(..PLUGINS_HEAD_LEN)?;
        if head[..8] != PLUGINS_MAGIC || u16::from_le_bytes([head[8], head[9]]) != PLUGINS_FORMAT {
            return None;
        }
        let len = usize::from(u16::from_le_bytes([head[10], head[11]]));
        (len <= PLUGINS_MAX_LEN).then_some(len)
    }

    /// The list at the start of `bytes` (its head, then its text).
    pub fn read(bytes: &[u8]) -> Option<Self> {
        let text = Self::text_of(bytes)?;
        Some(Self::new(String::from(text)))
    }

    /// The list at the start of `bytes` kept in the program, without
    /// copying it (the firmware's own).
    pub fn read_static(bytes: &'static [u8]) -> Option<Self> {
        Self::text_of(bytes).map(Self::new)
    }

    fn text_of(bytes: &[u8]) -> Option<&str> {
        let len = Self::text_len(bytes)?;
        let text = bytes.get(PLUGINS_HEAD_LEN..PLUGINS_HEAD_LEN + len)?;
        core::str::from_utf8(text).ok()
    }

    /// The list's bytes, head and text, for `crates`; `None` if the text
    /// is longer than [`PLUGINS_MAX_LEN`], which readers refuse.
    pub fn record<'a>(crates: impl IntoIterator<Item = PluginCrate<'a>>) -> Option<Vec<u8>> {
        let mut text = String::new();
        for c in crates {
            text.push_str(c.name);
            text.push(' ');
            text.push_str(c.version);
            text.push('\n');
        }
        if text.len() > PLUGINS_MAX_LEN {
            return None;
        }
        let mut bytes = Vec::with_capacity(PLUGINS_HEAD_LEN + text.len());
        bytes.extend_from_slice(&PLUGINS_MAGIC);
        bytes.extend_from_slice(&PLUGINS_FORMAT.to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(text.len()).ok()?.to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
        Some(bytes)
    }

    /// The text, one `name version` a line.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The crates, in the list's order.
    pub fn iter(&self) -> impl Iterator<Item = PluginCrate<'_>> {
        self.text.lines().filter_map(|line| {
            let (name, version) = line.trim().split_once(' ')?;
            Some(PluginCrate {
                name,
                version: version.trim(),
            })
        })
    }

    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    /// The version of the crate `name`, if it is built in.
    pub fn version(&self, name: &str) -> Option<&str> {
        self.iter().find(|c| c.name == name).map(|c| c.version)
    }

    /// The crates of this list that `other` does not have: what an update
    /// with `other` leaves out.
    pub fn missing_from<'a>(&'a self, other: &'a Plugins) -> impl Iterator<Item = &'a str> {
        self.iter()
            .map(|c| c.name)
            .filter(|name| other.version(name).is_none())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;

    fn crates() -> [PluginCrate<'static>; 2] {
        [
            PluginCrate {
                name: "paperpaw-plugin-stats",
                version: "0.4.0",
            },
            PluginCrate {
                name: "paperpaw-plugin-chess",
                version: "1.2.0",
            },
        ]
    }

    #[test]
    fn a_plugin_crate_names_its_plugin_id() {
        let id = |name| PluginCrate { name, version: "1" }.plugin_id();
        assert_eq!(id("paperpaw-plugin-stats"), Some("stats"));
        assert_eq!(id("paperpaw-plugin-hyphenation-de"), Some("hyphenation-de"));
        assert_eq!(id("paperpaw-plugin-"), None);
        assert_eq!(id("paperpaw-stats"), None);
        assert_eq!(id("other-plugin-stats"), None);
        assert_eq!(id("paperpaw-plugin-Stats"), None);
        assert_eq!(id("paperpaw-plugin-a.b"), None);
        let long = std::format!("paperpaw-plugin-{}", "x".repeat(64));
        assert_eq!(id(&long).map(str::len), Some(64));
        let longer = std::format!("paperpaw-plugin-{}", "x".repeat(65));
        assert_eq!(id(&longer), None);
    }

    #[test]
    fn a_list_reads_back() {
        let mut bytes = Plugins::record(crates()).unwrap();
        assert_eq!(&bytes[..8], b"PPlugins");
        bytes.extend_from_slice(b"what follows in the image");
        let list = Plugins::read(&bytes).unwrap();
        assert_eq!(list.iter().collect::<Vec<_>>(), crates());
        assert_eq!(list.version("paperpaw-plugin-chess"), Some("1.2.0"));
        assert_eq!(list.version("paperpaw-plugin-x"), None);
        assert_eq!(
            Plugins::text_len(&bytes),
            Some("paperpaw-plugin-stats 0.4.0\npaperpaw-plugin-chess 1.2.0\n".len())
        );
    }

    #[test]
    fn older_images_have_none() {
        assert_eq!(Plugins::read(&[0; 64]), None);
        assert_eq!(Plugins::read(b"PPlugins"), None);
        let mut newer = Plugins::record(crates()).unwrap();
        newer[8] = 2;
        assert_eq!(Plugins::read(&newer), None);
        let mut cut = Plugins::record(crates()).unwrap();
        cut.truncate(20);
        assert_eq!(Plugins::read(&cut), None);
    }

    #[test]
    fn a_list_too_long_to_read_is_not_written() {
        let name = std::format!("paperpaw-plugin-{}", "x".repeat(100));
        let many = || {
            core::iter::repeat_n(
                PluginCrate {
                    name: &name,
                    version: "0.4.0",
                },
                200,
            )
        };
        assert_eq!(Plugins::record(many()), None);
        let fits = Plugins::record(many().take(100)).unwrap();
        assert_eq!(Plugins::read(&fits).unwrap().iter().count(), 100);
    }

    #[test]
    fn what_an_update_leaves_out() {
        let running = Plugins::new("paperpaw-plugin-stats 0.4.0\npaperpaw-plugin-chess 1.2.0\n");
        let update = Plugins::new("paperpaw-plugin-stats 0.5.0\npaperpaw-plugin-new 0.5.0\n");
        assert_eq!(
            running.missing_from(&update).collect::<Vec<_>>(),
            ["paperpaw-plugin-chess"]
        );
        assert!(Plugins::default().is_empty());
    }
}
