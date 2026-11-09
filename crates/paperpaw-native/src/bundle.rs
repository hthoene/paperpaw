//! The web flasher's bundle of plugins (ADR 0024): the plugins a reader
//! chose, each with its files (a data plugin's folder, a code plugin's
//! `code.ppn` for this device and firmware), written with the firmware:
//! at the start of the other app partition, empty on a first flash, where
//! the firmware says it looks there (room for the whole store), else
//! behind the firmware's image in the app partition it goes to. The
//! firmware installs it onto the card at its first start, then erases it. The format is
//! the web flasher's (paperpaw.org, `static/flash/bundle.js`); this reader
//! takes what it writes and nothing else.
//!
//! Format 1, numbers little-endian, offsets from the bundle's first byte:
//!
//! | Bytes | What |
//! |-------|------|
//! | 0..4 | `PPNB` |
//! | 4..6 | the format, 1 |
//! | 6..8 | P, the number of plugins (1 to 256) |
//! | 8..12 | F, the number of files (P to 4096) |
//! | 12..16 | the bundle's length, to the end of the last file |
//! | 16..48 | the SHA-256 the app image ends with: the image it came with |
//! | 48..80 | the SHA-256 of bytes 80 to the bundle's length |
//! | 80.. | P plugins of 16 bytes, in the order to install |
//! | then | F files of 48 bytes, by plugin in that order |
//! | then | the texts, then 0 to 3 zero bytes |
//! | then | the files' bytes, each from a multiple of 4 |
//!
//! A plugin: the offset of its id (4 bytes), of its version (4), their
//! lengths (2 and 2), its number of files (4, at least 1: the next ones).
//! A file: the offset of its path (4), its length (2), 2 zero bytes, the
//! offset of its bytes (4), their size (4), their SHA-256 (32). The texts
//! are ASCII, back to back: each plugin's id and version, then every
//! file's path. Ids, versions and paths follow the device's rules for
//! installing plugins (as `paperpaw_sdk::install` has them); every plugin
//! has a `plugin.ini`. Every offset is the one the layout gives, padding
//! is zero: a bundle has one form only, and [`Bundle::read`] refuses any
//! other.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use paperpaw_digest::sha256::Sha256;

/// The bundle's first bytes.
pub const BUNDLE_MAGIC: [u8; 4] = *b"PPNB";
/// The format this crate reads.
pub const BUNDLE_FORMAT: u16 = 1;
const HEAD_LEN: u32 = 80;
const PLUGIN_LEN: u32 = 16;
const FILE_LEN: u32 = 48;
const MAX_PLUGINS: u16 = 256;
const MAX_FILES: u32 = 4096;
/// The longest plugin id, version and path.
const MAX_ID: usize = 40;
const MAX_VERSION: usize = 32;
const MAX_PATH: usize = 100;

/// What is wrong with a bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BundleProblem {
    /// There is no bundle.
    NotABundle,
    /// A bundle of a newer format than this one reads.
    NewerFormat,
    /// It was written behind another image.
    OtherImage,
    /// Its bytes cannot all be read, or it is longer than its room.
    Length,
    /// It was changed or damaged: its SHA-256 does not hold.
    Damaged,
    /// Its tables or texts are not as the format lays them out.
    Layout,
    /// An id, version or path is not one the device installs.
    Name,
    /// There is not memory enough for its tables.
    TooLarge,
}

impl fmt::Display for BundleProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BundleProblem::NotABundle => "no bundle",
            BundleProblem::NewerFormat => "a bundle of a newer format",
            BundleProblem::OtherImage => "written behind another image",
            BundleProblem::Length => "not as long as it says",
            BundleProblem::Damaged => "changed or damaged",
            BundleProblem::Layout => "not laid out as the format says",
            BundleProblem::Name => "an id, version or path the device does not install",
            BundleProblem::TooLarge => "too large for this device's memory",
        })
    }
}

impl core::error::Error for BundleProblem {}

/// Where a bundle's bytes are read from: `read(at, buf)` fills `buf`
/// from offset `at` of the bundle, `false` when it cannot.
pub trait Source {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> bool;
}

/// A bundle in memory (and the bytes behind it, as flash has them).
impl Source for &[u8] {
    fn read(&mut self, at: u32, buf: &mut [u8]) -> bool {
        let at = at as usize;
        match self.get(at..at + buf.len()) {
            Some(bytes) => {
                buf.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }
}

/// A plugin in a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundlePlugin {
    pub id: String,
    pub version: String,
    pub files: Vec<BundleFile>,
}

/// A file of a plugin in a bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    /// Where it goes in the plugin's folder.
    pub path: String,
    /// Where its bytes are in the bundle.
    pub at: u32,
    pub size: u32,
    pub sha256: [u8; 32],
}

/// A bundle, checked whole (see the [module documentation](self)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Its bytes.
    pub len: u32,
    /// Its plugins, in the order to install them.
    pub plugins: Vec<BundlePlugin>,
}

impl Bundle {
    /// The bundle `source` holds, checked whole against the format and
    /// against `image` (the SHA-256 the app image it must lie behind ends
    /// with), in `room` bytes at most.
    #[expect(
        clippy::too_many_lines,
        reason = "longer than the limit from before it was set; split it when it is next changed"
    )]
    pub fn read(
        source: &mut dyn Source,
        image: &[u8; 32],
        room: u32,
    ) -> Result<Bundle, BundleProblem> {
        let mut head = [0u8; HEAD_LEN as usize];
        if !source.read(0, &mut head) || head[..4] != BUNDLE_MAGIC {
            return Err(BundleProblem::NotABundle);
        }
        if le16(&head, 4) != BUNDLE_FORMAT {
            return Err(BundleProblem::NewerFormat);
        }
        let plugins = le16(&head, 6);
        let files = le32(&head, 8);
        let len = le32(&head, 12);
        if !(1..=MAX_PLUGINS).contains(&plugins) || files < u32::from(plugins) || files > MAX_FILES
        {
            return Err(BundleProblem::Layout);
        }
        let files_at = HEAD_LEN + PLUGIN_LEN * u32::from(plugins);
        let texts_at = files_at + FILE_LEN * files;
        if len < texts_at || len > room {
            return Err(BundleProblem::Length);
        }
        if head[16..48] != image[..] {
            return Err(BundleProblem::OtherImage);
        }
        if digest(source, HEAD_LEN, len)? != head[48..80] {
            return Err(BundleProblem::Damaged);
        }
        let mut layout = Layout {
            source,
            next_text: texts_at,
            texts_at,
            len,
        };
        let mut read = Vec::new();
        read.try_reserve_exact(usize::from(plugins))
            .map_err(|_| BundleProblem::TooLarge)?;
        let mut file_index = 0u32;
        let mut entries = Vec::new();
        for i in 0..u32::from(plugins) {
            let mut entry = [0u8; PLUGIN_LEN as usize];
            layout.bytes(HEAD_LEN + PLUGIN_LEN * i, &mut entry)?;
            let count = le32(&entry, 12);
            if count < 1 || file_index + count > files {
                return Err(BundleProblem::Layout);
            }
            entries.push((entry, count));
            file_index += count;
        }
        if file_index != files {
            return Err(BundleProblem::Layout);
        }
        // The texts in the layout's order: ids and versions, then paths.
        for (entry, _) in &entries {
            let id = layout.text(le32(entry, 0), le16(entry, 8))?;
            let version = layout.text(le32(entry, 4), le16(entry, 10))?;
            // `core` is the host's own id: no bundle installs a plugin by it.
            if !is_plugin_id(&id)
                || id == "core"
                || !is_version(&version)
                || read.iter().any(|p: &BundlePlugin| p.id == id)
            {
                return Err(BundleProblem::Name);
            }
            read.push(BundlePlugin {
                id,
                version,
                files: Vec::new(),
            });
        }
        // Then every file's path; its bytes after the texts, each from a
        // multiple of 4.
        let mut tables = Vec::new();
        tables
            .try_reserve_exact(files as usize)
            .map_err(|_| BundleProblem::TooLarge)?;
        for i in 0..files {
            let mut entry = [0u8; FILE_LEN as usize];
            layout.bytes(files_at + FILE_LEN * i, &mut entry)?;
            let path = layout.text(le32(&entry, 0), le16(&entry, 4))?;
            if le16(&entry, 6) != 0 {
                return Err(BundleProblem::Layout);
            }
            tables.push((entry, path));
        }
        let mut next_data = layout.next_text;
        let mut tables = tables.into_iter();
        for ((_, count), plugin) in entries.iter().zip(read.iter_mut()) {
            plugin
                .files
                .try_reserve_exact(*count as usize)
                .map_err(|_| BundleProblem::TooLarge)?;
            for (entry, path) in tables.by_ref().take(*count as usize) {
                let at = le32(&entry, 8);
                let size = le32(&entry, 12);
                if at != next_data.next_multiple_of(4)
                    || at.checked_add(size).is_none_or(|end| end > len)
                {
                    return Err(BundleProblem::Layout);
                }
                layout.zeros(next_data, at)?;
                next_data = at + size;
                if !is_safe_path(&path)
                    || plugin
                        .files
                        .iter()
                        .any(|f| f.path.eq_ignore_ascii_case(&path))
                {
                    return Err(BundleProblem::Name);
                }
                let mut sha256 = [0; 32];
                sha256.copy_from_slice(&entry[16..48]);
                plugin.files.push(BundleFile {
                    path,
                    at,
                    size,
                    sha256,
                });
            }
            if !plugin.files.iter().any(|f| f.path == "plugin.ini") {
                return Err(BundleProblem::Name);
            }
        }
        if next_data != len {
            return Err(BundleProblem::Layout);
        }
        Ok(Bundle { len, plugins: read })
    }
}

/// Reads a bundle's tables and texts, each where the layout puts it.
struct Layout<'a> {
    source: &'a mut dyn Source,
    /// Where the next text must begin.
    next_text: u32,
    texts_at: u32,
    len: u32,
}

impl Layout<'_> {
    fn bytes(&mut self, at: u32, buf: &mut [u8]) -> Result<(), BundleProblem> {
        if self.source.read(at, buf) {
            Ok(())
        } else {
            Err(BundleProblem::Length)
        }
    }

    /// The text at `at`, `len` bytes: the next of the texts, ASCII.
    fn text(&mut self, at: u32, len: u16) -> Result<String, BundleProblem> {
        let end = at + u32::from(len);
        if at != self.next_text || at < self.texts_at || end > self.len || len > 255 {
            return Err(BundleProblem::Layout);
        }
        let mut buf = [0u8; 255];
        let buf = &mut buf[..usize::from(len)];
        self.bytes(at, buf)?;
        self.next_text = end;
        let text = core::str::from_utf8(buf).map_err(|_| BundleProblem::Name)?;
        if !text.is_ascii() {
            return Err(BundleProblem::Name);
        }
        Ok(String::from(text))
    }

    /// Whether the bytes from `from` to `to` are all zero.
    fn zeros(&mut self, from: u32, to: u32) -> Result<(), BundleProblem> {
        let mut buf = [0u8; 4];
        let buf = &mut buf[..(to - from) as usize];
        self.bytes(from, buf)?;
        if buf.iter().all(|&b| b == 0) {
            Ok(())
        } else {
            Err(BundleProblem::Layout)
        }
    }
}

/// The SHA-256 of the bundle's bytes from `from` to `to`.
fn digest(source: &mut dyn Source, from: u32, to: u32) -> Result<[u8; 32], BundleProblem> {
    let mut sha = Sha256::new();
    let mut buf = [0u8; 512];
    let mut at = from;
    while at < to {
        let len = (to - at).min(buf.len() as u32) as usize;
        if !source.read(at, &mut buf[..len]) {
            return Err(BundleProblem::Length);
        }
        sha.update(&buf[..len]);
        at += len as u32;
    }
    Ok(sha.finish())
}

/// Whether `id` can be a plugin's id and its folder's name: 1 to 40
/// lower-case letters, digits and `-`, not starting with `-` (as
/// `paperpaw_sdk::install::is_plugin_name`).
pub fn is_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && !id.starts_with('-')
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// Whether `path` stays within a plugin's folder: plain names, `/`
/// between them, none starting with `.` (as
/// `paperpaw_sdk::install::is_safe_path`).
pub fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        })
}

/// Whether `version` can be a plugin's version: 1 to 32 printable ASCII
/// characters, no spaces.
fn is_version(version: &str) -> bool {
    (1..=MAX_VERSION).contains(&version.len())
        && version.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

fn le16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn le32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The web flasher's own test bundle (paperpaw.org,
    /// `tests/fixtures/flash/bundle-1.bin`): plugin `a`, version `1`, its
    /// `plugin.ini` holding `x`, behind the image whose SHA-256 is 1 to 32.
    const FIXTURE: &[u8] = include_bytes!("../tests/data/bundle-1.bin");

    fn image() -> [u8; 32] {
        core::array::from_fn(|i| i as u8 + 1)
    }

    /// A plugin to build into a bundle: its id, version and files (path
    /// and bytes).
    type Plugin<'a> = (&'a str, &'a str, &'a [(&'a str, &'a [u8])]);
    /// A change to a bundle, and what reading it then says.
    type Case = (&'static dyn Fn(&mut Vec<u8>), BundleProblem);

    /// A bundle as the flasher lays it out, of `plugins`.
    fn build(image: &[u8; 32], plugins: &[Plugin<'_>]) -> Vec<u8> {
        let files: usize = plugins.iter().map(|p| p.2.len()).sum();
        let texts_at = HEAD_LEN as usize + 16 * plugins.len() + 48 * files;
        let mut texts = Vec::new();
        let mut place = |text: &str| {
            let at = texts_at + texts.len();
            texts.extend_from_slice(text.as_bytes());
            (at as u32, text.len() as u16)
        };
        let ids: Vec<_> = plugins.iter().map(|p| (place(p.0), place(p.1))).collect();
        let paths: Vec<_> = plugins
            .iter()
            .flat_map(|p| p.2.iter())
            .map(|f| place(f.0))
            .collect();
        let mut at = texts_at + texts.len();
        let data: Vec<usize> = plugins
            .iter()
            .flat_map(|p| p.2.iter())
            .map(|f| {
                at = at.next_multiple_of(4);
                at += f.1.len();
                at - f.1.len()
            })
            .collect();
        let mut bundle = alloc::vec![0u8; at];
        bundle[..4].copy_from_slice(&BUNDLE_MAGIC);
        bundle[4..6].copy_from_slice(&1u16.to_le_bytes());
        bundle[6..8].copy_from_slice(&(plugins.len() as u16).to_le_bytes());
        bundle[8..12].copy_from_slice(&(files as u32).to_le_bytes());
        bundle[12..16].copy_from_slice(&(at as u32).to_le_bytes());
        bundle[16..48].copy_from_slice(image);
        for (i, (p, (id, version))) in plugins.iter().zip(&ids).enumerate() {
            let e = HEAD_LEN as usize + 16 * i;
            bundle[e..e + 4].copy_from_slice(&id.0.to_le_bytes());
            bundle[e + 4..e + 8].copy_from_slice(&version.0.to_le_bytes());
            bundle[e + 8..e + 10].copy_from_slice(&id.1.to_le_bytes());
            bundle[e + 10..e + 12].copy_from_slice(&version.1.to_le_bytes());
            bundle[e + 12..e + 16].copy_from_slice(&(p.2.len() as u32).to_le_bytes());
        }
        let all: Vec<_> = plugins.iter().flat_map(|p| p.2.iter()).collect();
        for (i, file) in all.iter().enumerate() {
            let e = HEAD_LEN as usize + 16 * plugins.len() + 48 * i;
            bundle[e..e + 4].copy_from_slice(&paths[i].0.to_le_bytes());
            bundle[e + 4..e + 6].copy_from_slice(&paths[i].1.to_le_bytes());
            bundle[e + 8..e + 12].copy_from_slice(&(data[i] as u32).to_le_bytes());
            bundle[e + 12..e + 16].copy_from_slice(&(file.1.len() as u32).to_le_bytes());
            bundle[e + 16..e + 48].copy_from_slice(&paperpaw_digest::sha256::digest(file.1));
            bundle[data[i]..data[i] + file.1.len()].copy_from_slice(file.1);
        }
        bundle[texts_at..texts_at + texts.len()].copy_from_slice(&texts);
        seal(&mut bundle);
        bundle
    }

    /// Makes the bundle's own SHA-256 right, to reach the checks behind it.
    fn seal(bundle: &mut [u8]) {
        let len = le32(bundle, 12) as usize;
        let sha = paperpaw_digest::sha256::digest(&bundle[80..len]);
        bundle[48..80].copy_from_slice(&sha);
    }

    fn read(bundle: &[u8]) -> Result<Bundle, BundleProblem> {
        Bundle::read(&mut &bundle[..], &image(), u32::MAX)
    }

    #[test]
    fn the_flashers_bundle_reads_as_it_wrote_it() {
        let bundle = read(FIXTURE).unwrap();
        assert_eq!(bundle.len, 157);
        assert_eq!(bundle.plugins.len(), 1);
        let a = &bundle.plugins[0];
        assert_eq!((a.id.as_str(), a.version.as_str()), ("a", "1"));
        assert_eq!(a.files.len(), 1);
        let ini = &a.files[0];
        assert_eq!(
            (ini.path.as_str(), ini.at, ini.size),
            ("plugin.ini", 156, 1)
        );
        assert_eq!(ini.sha256, paperpaw_digest::sha256::digest(b"x"));
        assert_eq!(
            build(&image(), &[("a", "1", &[("plugin.ini", b"x")])]),
            FIXTURE,
            "built alike"
        );
    }

    #[test]
    fn several_plugins_with_files_of_any_size() {
        let code: Vec<u8> = (0..70_001u32).map(|i| (i * 31) as u8).collect();
        let plugins: &[Plugin<'_>] = &[
            (
                "reading-nook",
                "1.0.0",
                &[
                    ("plugin.ini", b"[plugin]\n"),
                    ("presets.ini", b"[calm]\n"),
                    ("i18n/de.ini", b""),
                ],
            ),
            (
                "hello",
                "1.0.0-rc.1",
                &[("plugin.ini", b"[plugin]\n"), ("code.ppn", &code)],
            ),
        ];
        let bytes = build(&image(), plugins);
        // As in flash: erased bytes behind it.
        let mut flash = bytes.clone();
        flash.resize(bytes.len() + 4096, 0xFF);
        let bundle = read(&flash).unwrap();
        assert_eq!(bundle.len as usize, bytes.len());
        let names: Vec<Vec<&str>> = bundle
            .plugins
            .iter()
            .map(|p| p.files.iter().map(|f| f.path.as_str()).collect())
            .collect();
        assert_eq!(
            names,
            [
                ["plugin.ini", "presets.ini", "i18n/de.ini"].as_slice(),
                &["plugin.ini", "code.ppn"]
            ]
        );
        let code_file = &bundle.plugins[1].files[1];
        assert_eq!(code_file.at % 4, 0);
        assert_eq!(
            &bytes[code_file.at as usize..][..code_file.size as usize],
            &code[..]
        );
        assert_eq!(
            Bundle::read(&mut &flash[..], &[0; 32], u32::MAX),
            Err(BundleProblem::OtherImage)
        );
        assert_eq!(
            Bundle::read(&mut &flash[..], &image(), bytes.len() as u32 - 1),
            Err(BundleProblem::Length),
            "longer than its room"
        );
    }

    #[test]
    fn what_is_not_the_one_form_is_refused() {
        let good = FIXTURE.to_vec();
        assert_eq!(
            read(&[0xFF; 100]),
            Err(BundleProblem::NotABundle),
            "erased flash"
        );
        assert_eq!(read(&good[..100]), Err(BundleProblem::Length), "cut short");
        let mut damaged = good.clone();
        damaged[150] ^= 1;
        assert_eq!(read(&damaged), Err(BundleProblem::Damaged));
        // Each changed, with the bundle's SHA-256 made right again.
        let cases: [Case; 9] = [
            (&|b| b[4] = 2, BundleProblem::NewerFormat),
            (&|b| b[6] = 0, BundleProblem::Layout),
            (&|b| b[8] = 0, BundleProblem::Layout),
            (&|b| b[80] = 145, BundleProblem::Layout),
            (&|b| b[102] = 1, BundleProblem::Layout),
            (&|b| b[104] = 152, BundleProblem::Layout),
            (&|b| b[144] = b'A', BundleProblem::Name),
            (&|b| b[146] = b'.', BundleProblem::Name),
            (&|b| b[150] = b'X', BundleProblem::Name),
        ];
        for (i, (change, problem)) in cases.iter().enumerate() {
            let mut bundle = good.clone();
            change(&mut bundle);
            seal(&mut bundle);
            assert_eq!(read(&bundle), Err(*problem), "case {i}");
        }
        let no_ini = build(&image(), &[("a", "1", &[("readme.txt", b"x")])]);
        assert_eq!(read(&no_ini), Err(BundleProblem::Name));
        let twice = build(
            &image(),
            &[
                ("a", "1", &[("plugin.ini", b"x")]),
                ("a", "1", &[("plugin.ini", b"x")]),
            ],
        );
        assert_eq!(read(&twice), Err(BundleProblem::Name));
        let host = build(&image(), &[("core", "1", &[("plugin.ini", b"x")])]);
        assert_eq!(read(&host), Err(BundleProblem::Name), "the host's own id");
        let like = build(&image(), &[("core-tools", "1", &[("plugin.ini", b"x")])]);
        assert!(read(&like).is_ok());
        let same_path = build(
            &image(),
            &[("a", "1", &[("plugin.ini", b"x"), ("PLUGIN.ini", b"y")])],
        );
        assert_eq!(
            read(&same_path),
            Err(BundleProblem::Name),
            "the card ignores case"
        );
        // Padding that is not zero.
        let mut padded = build(
            &image(),
            &[("a", "1", &[("plugin.ini", b"x"), ("b", b"y")])],
        );
        let pad = le32(&padded, 104 + 48) as usize - 1;
        padded[pad] = 1;
        seal(&mut padded);
        assert_eq!(read(&padded), Err(BundleProblem::Layout));
    }

    #[test]
    fn names_follow_the_install_rules() {
        assert!(is_plugin_id("reading-nook") && is_plugin_id("a1"));
        for id in ["", "-a", "A", "a.b", "a_b", &"a".repeat(41)] {
            assert!(!is_plugin_id(id), "{id}");
        }
        assert!(is_safe_path("i18n/de.ini") && is_safe_path("fonts/Book_Antiqua-Bold.ttf"));
        for path in [
            "",
            "/a",
            "a//b",
            "../a",
            ".hidden",
            "a/.b",
            "a b",
            "a\\b",
            &"a".repeat(101),
        ] {
            assert!(!is_safe_path(path), "{path}");
        }
        assert!(is_version("1.0.0-rc.1") && !is_version("1 0") && !is_version(""));
    }
}
