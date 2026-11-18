//! A community plugin source (#214, #318): a plugin library of one's own,
//! made of files any web server serves as they are (GitHub Pages, say), in
//! the format a reader's store reads (`plugins/store/src/native_sources.rs`
//! and `native_list.rs`).
//!
//! [`pack`] makes it from a folder that holds, for each plugin, its folder
//! (named as its id, with `plugin.ini`, and `store/store.ini` for what the
//! list says of it) and, for a plugin with code, its builds beside it
//! (`<id>-<version>-<device>.ppn`, signed with the source's key):
//!
//! ```text
//! plugins.json                         the list, every plugin and its builds
//! plugins.json.sig                     the source's key and its signature
//! plugins/<id>.json                    a plugin's page: its files, its key
//! plugins/<id>/<version>/files/...     the files a reader installs
//! plugins/<id>/<version>/<id>-<version>-<device>.ppn
//! ```
//!
//! A reader asks for the list with its device and language in the query
//! (`?device=x4&lang=en`), which such a server passes over: the list names
//! the builds of every device and the reader takes its own. The signature
//! file is the key (64 hexadecimal digits) and, on the next line, the
//! Ed25519 signature (128) over [`DOMAIN`] and the list's SHA-256
//! ([`sign`]). A reader keeps the key of the first list it reads of a source
//! and reads no list another key signed; it installs a plugin's code only
//! where the page names that key as its author's and the build is signed
//! with it, so [`pack`] refuses a build that is not.

use std::path::{Path, PathBuf};

use paperpaw_digest::{hex, sha256};
use paperpaw_native::signature::{public_key, sign_message, verify_message};

use crate::ini::{self, Line};
use crate::rules::{Rules, is_version};

/// What a source's signature covers before the list's SHA-256: the store's
/// `native_sources::DOMAIN`.
pub const DOMAIN: &[u8] = b"PaperPaw plugin source\0";
/// The format of the list and of a page (`native_model::FORMAT`).
pub const FORMAT: u32 = 2;
/// The largest list a reader takes of a community source, and the largest
/// page (`native_sources::MAX_SOURCE_LIST`, `native_load::MAX_PAGE_FILE`).
pub const MAX_LIST: usize = 512 * 1024;
/// The readers a build is listed for, by the names plugin libraries call
/// them; the X3 is served as `x4`.
pub const DEVICES: [&str; 3] = ["x4", "x4-pro", "x4-classic"];
/// The names a plugin's code has in its folder (`paperpaw_sdk::native::
/// CODE_FILES`): builds are listed apart, never among the folder's files.
const CODE_FILES: [&str; 5] = [
    "code.ppn",
    "code.so",
    "code-host.dll",
    "code-host.so",
    "code-host.dylib",
];

/// What the list says of every plugin and cannot be read from its folder.
#[derive(Debug, Clone, Copy, Default)]
pub struct About<'a> {
    /// Who makes the plugins, as the store shows it.
    pub author: &'a str,
    /// The day of this version, `yyyy-mm-dd`; empty where not said.
    pub date: &'a str,
}

/// A plugin of a source that was packed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub id: String,
    pub version: String,
    /// The readers it has a build for; none for a plugin without code.
    pub devices: Vec<&'static str>,
    /// The files a reader installs, its code not counted.
    pub files: usize,
}

/// A source that was packed: its key, and its plugins by id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub key: [u8; 32],
    pub plugins: Vec<Listed>,
}

/// The first digits of `key` as a reader shows them before it trusts a
/// source (`native_sources::fingerprint`): 16 digits in groups of four.
pub fn fingerprint(key: &[u8; 32]) -> String {
    let digits = hex::encode(&key[..8]);
    [&digits[..4], &digits[4..8], &digits[8..12], &digits[12..]].join(" ")
}

/// What the source's key signs of a list.
fn message(list: &[u8]) -> Vec<u8> {
    let mut message = DOMAIN.to_vec();
    message.extend_from_slice(&sha256::digest(list));
    message
}

/// The signature file of the list at `list`.
fn signature_path(list: &Path) -> PathBuf {
    let mut name = list.as_os_str().to_os_string();
    name.push(".sig");
    PathBuf::from(name)
}

fn failed(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

/// Signs the list at `list` with the key of `seed`: writes `<list>.sig` and
/// gives the public key.
///
/// # Errors
/// The list cannot be read, is larger than a reader takes, or the signature
/// file cannot be written.
pub fn sign(list: &Path, seed: &[u8; 32]) -> Result<[u8; 32], String> {
    let bytes = std::fs::read(list).map_err(|e| failed(list, e))?;
    if bytes.len() > MAX_LIST {
        return Err(format!(
            "{}: {} bytes, and a reader takes a source's list of at most {MAX_LIST}",
            list.display(),
            bytes.len()
        ));
    }
    let key = public_key(seed);
    let text = format!(
        "{}\n{}\n",
        hex::encode(&key),
        hex::encode(&sign_message(&message(&bytes), seed))
    );
    let signature = signature_path(list);
    std::fs::write(&signature, text).map_err(|e| failed(&signature, e))?;
    Ok(key)
}

/// Checks the list at `list` against `<list>.sig` as a reader does, and
/// gives the key that signed it.
///
/// # Errors
/// A file cannot be read, the signature file is not a key and a signature,
/// or the signature does not hold for this list.
pub fn check(list: &Path) -> Result<[u8; 32], String> {
    let bytes = std::fs::read(list).map_err(|e| failed(list, e))?;
    let signature = signature_path(list);
    let text = std::fs::read_to_string(&signature).map_err(|e| failed(&signature, e))?;
    let mut words = text.split_whitespace();
    let read = (|| {
        let key: [u8; 32] = hex::decode(words.next()?)?.try_into().ok()?;
        let signed: [u8; 64] = hex::decode(words.next()?)?.try_into().ok()?;
        words.next().is_none().then_some((key, signed))
    })();
    let Some((key, signed)) = read else {
        return Err(format!(
            "{}: not a key (64 hexadecimal digits) and a signature (128)",
            signature.display()
        ));
    };
    if bytes.len() > MAX_LIST || !verify_message(&key, &message(&bytes), &signed) {
        return Err(format!(
            "{}: the signature in {} does not hold for it: a reader would not read this list. \
             Sign it again after every change",
            list.display(),
            signature.display()
        ));
    }
    Ok(key)
}

/// `text` as a JSON string.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A value of an INI file as text: its quotes taken off, `\n` a line end.
fn value(lines: &[Line], section: &str, key: &str) -> String {
    let Some(line) = ini::get(lines, section, key) else {
        return String::new();
    };
    let text = line.value.as_str();
    match text
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        Some(inner) => inner.replace("\\n", "\n").replace("\\\"", "\""),
        None => text.to_string(),
    }
}

/// A file of a plugin as a page lists it.
struct Kept {
    /// Its path in the plugin's folder.
    path: String,
    size: u64,
    sha256: String,
}

/// A build as the list and a page name it.
struct Build {
    device: &'static str,
    /// Its file's name in the source, beside the plugin's files.
    name: String,
    abi: String,
    size: u64,
    sha256: String,
    image_len: u32,
}

/// A plugin read from its folder, with what was copied into the source.
struct Plugin {
    id: String,
    version: String,
    name: String,
    summary: String,
    description: String,
    group: String,
    licence: String,
    sdk: String,
    formats: String,
    source: String,
    repository: String,
    devices: Vec<String>,
    files: Vec<Kept>,
    builds: Vec<Build>,
}

/// Whether `path` is one a reader installs: plain names of letters, digits,
/// `.`, `_` and `-` joined by `/`, none starting with a dot, at most
/// `max` bytes (`paperpaw_sdk::install::is_safe_path`).
fn safe(path: &str, max: usize) -> bool {
    !path.is_empty()
        && path.len() <= max
        && path.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        })
}

/// The files below `dir`, by their paths from `folder`, sorted; hidden ones
/// and the top's `store/` (what only a store reads) left out.
fn walk(folder: &Path, dir: &Path, found: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| failed(dir, e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || (dir == folder && name == "store") {
            continue;
        }
        if path.is_dir() {
            walk(folder, &path, found)?;
        } else if let Ok(rest) = path.strip_prefix(folder) {
            let parts: Vec<_> = rest
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            found.push(parts.join("/"));
        }
    }
    Ok(())
}

/// Copies the files of the plugin in `folder` that a reader installs to
/// `<out>/plugins/<id>/<version>/files` and says what a page lists of each.
fn files(
    folder: &Path,
    out: &Path,
    id: &str,
    version: &str,
    rules: &Rules,
) -> Result<Vec<Kept>, String> {
    let mut paths = Vec::new();
    walk(folder, folder, &mut paths)?;
    let to = out.join("plugins").join(id).join(version).join("files");
    let mut files = Vec::new();
    for path in paths {
        let from = folder.join(&path);
        if !safe(&path, rules.max_path) {
            return Err(format!(
                "{}: a reader installs only paths of letters, digits, `.`, `_` and `-`, at most \
                 {} bytes",
                from.display(),
                rules.max_path
            ));
        }
        if CODE_FILES
            .iter()
            .any(|code| path.eq_ignore_ascii_case(code))
        {
            return Err(format!(
                "{}: the plugin's code is no file of its folder: put each build beside the \
                 folder as {id}-{version}-<device>.ppn",
                from.display()
            ));
        }
        let bytes = std::fs::read(&from).map_err(|e| failed(&from, e))?;
        let kept = to.join(&path);
        if let Some(parent) = kept.parent() {
            std::fs::create_dir_all(parent).map_err(|e| failed(parent, e))?;
        }
        std::fs::write(&kept, &bytes).map_err(|e| failed(&kept, e))?;
        files.push(Kept {
            path,
            size: bytes.len() as u64,
            sha256: hex::encode(&sha256::digest(&bytes)),
        });
    }
    Ok(files)
}

/// Reads the plugin in `folder` and copies the files a reader installs into
/// the source at `out`.
fn plugin(folder: &Path, out: &Path, rules: &Rules) -> Result<Plugin, String> {
    let manifest_path = folder.join("plugin.ini");
    let manifest =
        ini::read(&std::fs::read_to_string(&manifest_path).map_err(|e| failed(&manifest_path, e))?);
    let store = std::fs::read_to_string(folder.join("store/store.ini"))
        .map(|text| ini::read(&text))
        .unwrap_or_default();
    let id = value(&manifest, "plugin", "id");
    let version = value(&manifest, "plugin", "version");
    let name = value(&manifest, "plugin", "name");
    let place = manifest_path.display();
    if !rules.valid_id(&id) || id == "core" {
        return Err(format!(
            "{place}: `{id}` is no plugin id (lower-case letters, digits and hyphens, at most {})",
            rules.max_id
        ));
    }
    if folder.file_name().and_then(|n| n.to_str()) != Some(id.as_str()) {
        return Err(format!(
            "{}: the folder of plugin `{id}` is named as its id",
            folder.display()
        ));
    }
    if !is_version(&version) {
        return Err(format!(
            "{place}: version `{version}` is not three numbers like 1.0.0"
        ));
    }
    if name.is_empty() {
        return Err(format!("{place}: no name"));
    }
    let files = files(folder, out, &id, &version, rules)?;
    let summary = match value(&store, "store", "summary") {
        summary if summary.is_empty() => value(&manifest, "plugin", "description"),
        summary => summary,
    };
    let devices = value(&store, "store", "devices")
        .split(',')
        .map(|device| device.trim().to_string())
        .filter(|device| !device.is_empty())
        // The X3 asks as the X4.
        .map(|device| if device == "x3" { "x4".into() } else { device })
        .collect();
    let formats = match value(&manifest, "plugin", "formats") {
        formats if formats.parse::<u32>().is_ok() => formats,
        _ => "1".into(),
    };
    let https = |text: String| Some(text).filter(|t| t.starts_with("https://"));
    let repository = https(value(&store, "source", "repository")).unwrap_or_default();
    Ok(Plugin {
        name,
        summary,
        description: value(&store, "store", "description"),
        group: value(&store, "store", "group"),
        licence: value(&store, "store", "licence"),
        sdk: value(&store, "store", "sdk"),
        formats,
        source: https(value(&store, "store", "source")).unwrap_or_else(|| repository.clone()),
        repository,
        devices,
        files,
        builds: Vec::new(),
        id,
        version,
    })
}

/// The chip a reader named `device` has, as a build's header names it.
fn chip(device: &str) -> paperpaw_native::Chip {
    if device == "x4" {
        paperpaw_native::Chip::Esp32C3
    } else {
        paperpaw_native::Chip::Esp32S3
    }
}

/// Takes the build `file` of `plugin` for `device` into the source: a plugin
/// file a reader of that device loads, signed with the source's key `key`.
fn build(
    file: &Path,
    plugin: &mut Plugin,
    device: &'static str,
    key: &[u8; 32],
    out: &Path,
    rules: &Rules,
) -> Result<(), String> {
    let bytes = std::fs::read(file).map_err(|e| failed(file, e))?;
    if bytes.len() as u64 > rules.max_build {
        return Err(failed(file, "larger than a build may be"));
    }
    let (header, signature) = crate::release::build(&bytes).map_err(|e| failed(file, e))?;
    if header.target != chip(device) {
        return Err(failed(
            file,
            format!(
                "built for the {:?}, which is not the {device}'s chip",
                header.target
            ),
        ));
    }
    match signature {
        Some(signature) if signature.key == *key => {}
        Some(_) => {
            return Err(failed(
                file,
                "signed with another key than the source's: a reader installs a source's code \
                 only where the key that signs its list signed it. Sign it with the same key \
                 (`paperpaw-sign sign-plugin <file> --key <key file>`)",
            ));
        }
        None => {
            return Err(failed(
                file,
                "not signed: sign it with the source's key (`paperpaw-sign sign-plugin <file> \
                 --key <key file>`)",
            ));
        }
    }
    let name = format!("{}-{}-{device}.ppn", plugin.id, plugin.version);
    let to = out.join("plugins").join(&plugin.id).join(&plugin.version);
    std::fs::create_dir_all(&to).map_err(|e| failed(&to, e))?;
    std::fs::write(to.join(&name), &bytes).map_err(|e| failed(&to.join(&name), e))?;
    plugin.builds.push(Build {
        device,
        name,
        abi: format!("{}.{}", header.abi_major, header.needs_minor),
        size: bytes.len() as u64,
        sha256: hex::encode(&sha256::digest(&bytes)),
        image_len: header.image_len,
    });
    Ok(())
}

/// The plugin as the list (`page` false) or its page says it; `base` is
/// where its files are from the address of the JSON written.
fn entry(plugin: &Plugin, about: About<'_>, key: &[u8; 32], page: bool, base: &str) -> String {
    let code = !plugin.builds.is_empty();
    let mut out = String::from("{");
    let field = |out: &mut String, name: &str, text: &str| {
        if !text.is_empty() {
            out.push_str(&format!("{}: {}, ", quoted(name), quoted(text)));
        }
    };
    field(&mut out, "id", &plugin.id);
    field(&mut out, "name", &plugin.name);
    field(&mut out, "summary", &plugin.summary);
    field(&mut out, "group", &plugin.group);
    field(&mut out, "kind", if code { "code" } else { "data" });
    field(&mut out, "author", about.author);
    field(&mut out, "licence", &plugin.licence);
    field(&mut out, "source", &plugin.source);
    field(&mut out, "repository", &plugin.repository);
    if page {
        field(&mut out, "how", &plugin.description);
        if code {
            field(&mut out, "author_key", &hex::encode(key));
        }
    }
    // Nobody but its publisher looked at a community source's plugin.
    out.push_str("\"unreviewed\": true, ");
    if !code && !plugin.devices.is_empty() {
        let devices: Vec<String> = plugin.devices.iter().map(|d| quoted(d)).collect();
        out.push_str(&format!("\"devices\": [{}], ", devices.join(", ")));
    }
    out.push_str("\"latest\": {");
    field(&mut out, "version", &plugin.version);
    field(&mut out, "sdk", &plugin.sdk);
    field(&mut out, "date", about.date);
    let size: u64 = plugin.files.iter().map(|file| file.size).sum();
    out.push_str(&format!(
        "\"formats\": {}, \"size\": {size}",
        plugin.formats
    ));
    if page {
        let files: Vec<String> = plugin
            .files
            .iter()
            .map(|file| {
                format!(
                    "\n   {{\"path\": {}, \"url\": {}, \"size\": {}, \"sha256\": {}}}",
                    quoted(&file.path),
                    quoted(&format!("{base}files/{}", file.path)),
                    file.size,
                    quoted(&file.sha256)
                )
            })
            .collect();
        out.push_str(&format!(", \"files\": [{}]", files.join(",")));
    }
    if code {
        let builds: Vec<String> = plugin
            .builds
            .iter()
            .map(|build| {
                format!(
                    "\n   {{\"device\": {}, \"abi\": {}, \"url\": {}, \"size\": {}, \
                     \"sha256\": {}, \"image_len\": {}}}",
                    quoted(build.device),
                    quoted(&build.abi),
                    quoted(&format!("{base}{}", build.name)),
                    build.size,
                    quoted(&build.sha256),
                    build.image_len
                )
            })
            .collect();
        out.push_str(&format!(", \"builds\": [{}]", builds.join(",")));
    }
    out.push_str("}}");
    out
}

/// Takes the builds among `names` in `input` into the source, each beside
/// its plugin's folder and named for its reader, and holds each plugin to
/// the files a reader installs at most.
fn builds(
    input: &Path,
    names: &[String],
    plugins: &mut [Plugin],
    key: &[u8; 32],
    out: &Path,
    rules: &Rules,
) -> Result<(), String> {
    // The builds, each beside its plugin's folder and named for its reader.
    for name in names.iter().filter(|name| name.ends_with(".ppn")) {
        let file = input.join(name);
        let named = plugins.iter_mut().find_map(|plugin| {
            let rest = name
                .strip_prefix(&format!("{}-{}-", plugin.id, plugin.version))?
                .strip_suffix(".ppn")?;
            let device = DEVICES.into_iter().find(|device| *device == rest)?;
            Some((plugin, device))
        });
        let Some((plugin, device)) = named else {
            return Err(failed(
                &file,
                format!(
                    "names no plugin of this folder and its reader: a build is \
                     <id>-<version>-<device>.ppn, the device one of {}",
                    DEVICES.join(", ")
                ),
            ));
        };
        build(&file, plugin, device, key, out, rules)?;
    }
    for plugin in plugins {
        // The X4 Classic runs the X4 Pro's code: one build serves both.
        if let Some(pro) = plugin.builds.iter().find(|b| b.device == "x4-pro")
            && !plugin.builds.iter().any(|b| b.device == "x4-classic")
        {
            let classic = Build {
                device: "x4-classic",
                name: pro.name.clone(),
                abi: pro.abi.clone(),
                sha256: pro.sha256.clone(),
                ..*pro
            };
            plugin.builds.push(classic);
        }
        // In the order of the readers, whatever the files' names sort to.
        plugin
            .builds
            .sort_by_key(|build| DEVICES.iter().position(|device| *device == build.device));
        let files = plugin.files.len() + usize::from(!plugin.builds.is_empty());
        if files > rules.max_files {
            return Err(format!(
                "plugin `{}` has {files} files with its code: a reader installs at most {}",
                plugin.id, rules.max_files
            ));
        }
    }
    Ok(())
}

/// Writes the pages and the list of `plugins` into the source at `out`; the
/// list's path.
fn write(
    plugins: &[Plugin],
    about: About<'_>,
    key: &[u8; 32],
    out: &Path,
) -> Result<PathBuf, String> {
    let pages = out.join("plugins");
    std::fs::create_dir_all(&pages).map_err(|e| failed(&pages, e))?;
    let mut listed = Vec::new();
    for plugin in plugins {
        let base = format!("{}/{}/", plugin.id, plugin.version);
        let page = format!(
            "{{\"format\": {FORMAT}, \"plugin\": {}}}\n",
            entry(plugin, about, key, true, &base)
        );
        let path = pages.join(format!("{}.json", plugin.id));
        if page.len() > MAX_LIST {
            return Err(failed(&path, "larger than a reader takes a page"));
        }
        std::fs::write(&path, page).map_err(|e| failed(&path, e))?;
        listed.push(entry(
            plugin,
            about,
            key,
            false,
            &format!("plugins/{}/{}/", plugin.id, plugin.version),
        ));
    }
    let list = out.join("plugins.json");
    let text = format!(
        "{{\"format\": {FORMAT}, \"plugins\": [\n {}\n]}}\n",
        listed.join(",\n ")
    );
    std::fs::write(&list, text).map_err(|e| failed(&list, e))?;
    Ok(list)
}

/// Makes the source of the plugins in `input` in `out` and signs its list
/// with the key of `seed`: the files of the module's head, written over
/// those of an earlier run.
///
/// # Errors
/// What keeps a reader from listing or installing one of the plugins, with
/// what to change; or a file that cannot be read or written.
pub fn pack(input: &Path, out: &Path, seed: &[u8; 32], about: About<'_>) -> Result<Packed, String> {
    let rules = Rules::built_in();
    let key = public_key(seed);
    let mut names: Vec<String> = std::fs::read_dir(input)
        .map_err(|e| failed(input, e))?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut plugins = Vec::new();
    for name in &names {
        let folder = input.join(name);
        if folder.join("plugin.ini").is_file() {
            plugins.push(plugin(&folder, out, &rules)?);
        }
    }
    if plugins.is_empty() {
        return Err(format!(
            "{}: no plugin in it: it holds a folder for each plugin, named as its id, with its \
             plugin.ini",
            input.display()
        ));
    }
    builds(input, &names, &mut plugins, &key, out, &rules)?;
    let list = write(&plugins, about, &key, out)?;
    sign(&list, seed)?;
    Ok(Packed {
        key,
        plugins: plugins
            .into_iter()
            .map(|plugin| Listed {
                devices: plugin.builds.iter().map(|build| build.device).collect(),
                files: plugin.files.len(),
                id: plugin.id,
                version: plugin.version,
            })
            .collect(),
    })
}
