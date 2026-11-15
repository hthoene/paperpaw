//! The files of a release, as the site reads them from GitHub: by their
//! names, their sizes and, for the builds, their headers and signatures.
//! The release workflow makes them into a folder and checks it before it
//! releases anything.

use std::path::Path;

use paperpaw_native::header5::{self, Header5, Host};
use paperpaw_native::stream5::Relocator5;
use paperpaw_native::{Chip, MAGIC, Signature};

use crate::Problem;
use crate::repo::Plugin;
use crate::rules::Rules;

/// The chip of the reader named `device`.
fn chip(device: &str) -> Option<Chip> {
    match device {
        "x4" | "x3" => Some(Chip::Esp32C3),
        "x4-pro" | "x4-classic" => Some(Chip::Esp32S3),
        _ => None,
    }
}

/// A build's file (format 5, `abi/ppn5.md`) read as a reader of its own
/// chip and plugin interface reads it: the header, the digest, the words to
/// relocate. With its author's signature, checked, if it ends with one; or
/// what is wrong.
fn build(file: &[u8]) -> Result<(Header5, Option<Signature>), String> {
    let word = |at: usize| {
        file.get(at..at + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
    };
    let own = (file.get(..4) == Some(&MAGIC[..])).then_some(());
    if let Some(format) = own.and_then(|()| word(4)).filter(|f| *f < header5::FORMAT) {
        return Err(format!(
            "a plugin file of format {format}, made for a PaperPaw before plugin interface 7: \
             build it with this release's SDK and compilers"
        ));
    }
    let host = match word(6).and_then(Chip::from_id) {
        Some(Chip::Esp32S3) => Host::x4_pro(u16::MAX),
        _ => Host::x4(u16::MAX),
    };
    let host = Host {
        abi_major: word(8).unwrap_or(host.abi_major),
        ..host
    };
    let refused = |why: &dyn std::fmt::Debug| {
        format!("readers refuse it: {why:?}; make it again with `paperpaw-sign pack`")
    };
    let header = Header5::read(file, file.len() as u64, &host).map_err(|why| refused(&why))?;
    let end = header.file_len() as usize;
    let body = &file[..end];
    let mut scratch = vec![0u8; 4096];
    Relocator5::new(&header, body, header.base)
        .and_then(|relocator| relocator.verify(&mut scratch))
        .map_err(|why| refused(&why))?;
    if !header.signed {
        return Ok((header, None));
    }
    match Signature::read(&file[end..]) {
        Some(signature)
            if signature.signed_len as usize == end && signature.holds(body).is_ok() =>
        {
            Ok((header, Some(signature)))
        }
        _ => Err(refused(&"its author's signature does not hold")),
    }
}

/// Whether `tag` names `version` as the site takes it: `v1.0.0` or
/// `1.0.0`.
pub fn check_tag(tag: &str, version: &str) -> Vec<Problem> {
    let named = tag.strip_prefix('v').unwrap_or(tag);
    if named == version {
        Vec::new()
    } else {
        vec![Problem::error(
            "version",
            format!("tag {tag}"),
            format!("plugin.ini says version {version}: tag it v{version}"),
        )]
    }
}

/// Checks the release files in `dir` for `plugin`, tagged `tag`, its
/// builds for the plugin interface `interface` where it is known.
pub fn check(
    dir: &Path,
    plugin: &Plugin,
    tag: Option<&str>,
    interface: Option<u16>,
    rules: &Rules,
) -> Vec<Problem> {
    let mut problems = Vec::new();
    if let Some(tag) = tag {
        problems.extend(check_tag(tag, &plugin.version));
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        problems.push(Problem::error(
            "upload",
            dir.display().to_string(),
            "no such folder of release files",
        ));
        return problems;
    };
    let mut names: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let (id, version) = (&plugin.id, &plugin.version);
    let prefix = format!("{id}-{version}");
    let read: Vec<&String> = names.iter().filter(|n| n.starts_with(&prefix)).collect();
    if read.len() > rules.max_assets {
        problems.push(Problem::error(
            "upload",
            dir.display().to_string(),
            format!(
                "{} files: the site reads at most {}",
                read.len(),
                rules.max_assets
            ),
        ));
    }
    for name in names.iter().filter(|n| !n.starts_with(&prefix)) {
        problems.push(Problem::warning(
            "upload",
            name.clone(),
            format!("the site does not read it: its files begin with {prefix}"),
        ));
    }
    let size = |name: &str| std::fs::metadata(dir.join(name)).map_or(0, |m| m.len());
    // The plugin's folder, or the Workshop's file with all in it.
    let store_file = format!("{prefix}-store.zip");
    let package = [format!("{prefix}-upload.zip"), format!("{prefix}.zip")]
        .into_iter()
        .find(|n| names.contains(n));
    if names.contains(&store_file) {
        if size(&store_file) > rules.max_package * 2 {
            problems.push(Problem::error(
                "upload",
                store_file,
                "larger than the site takes",
            ));
        }
        return problems;
    }
    let Some(package) = package else {
        problems.push(Problem::error(
            "upload",
            dir.display().to_string(),
            format!("no {prefix}.zip: the release needs the plugin's folder as a ZIP"),
        ));
        return problems;
    };
    if size(&package) > rules.max_package {
        problems.push(Problem::error(
            "upload",
            package.clone(),
            format!("larger than {} MB", rules.max_package >> 20),
        ));
    }
    problems.extend(zip_check(dir, &package, plugin));
    let builds = builds(dir, &names, &prefix, interface, rules, &mut problems);
    match (plugin.code, builds == 0) {
        (true, true) => problems.push(Problem::error(
            "upload",
            dir.display().to_string(),
            format!(
                "a plugin with code needs a signed build for each reader: {prefix}-x4.ppn, ..."
            ),
        )),
        (false, false) => problems.push(Problem::error(
            "upload",
            dir.display().to_string(),
            "builds, but no Cargo.toml in the repository: the site would take it for a plugin \
             with code without its source",
        )),
        _ => {}
    }
    let source = format!("{prefix}-source.zip");
    if names.contains(&source) && size(&source) > rules.max_source {
        problems.push(Problem::error(
            "source",
            source,
            format!("larger than {} MB", rules.max_source >> 20),
        ));
    }
    problems
}

/// The builds among `names` (each `<prefix>-<device>.ppn`), each for the
/// reader its name says, of `interface`, signed, all by one key: how many
/// there are.
fn builds(
    dir: &Path,
    names: &[String],
    prefix: &str,
    interface: Option<u16>,
    rules: &Rules,
    problems: &mut Vec<Problem>,
) -> usize {
    // The builds, each for the reader its name says.
    let mut keys = Vec::new();
    let mut devices = Vec::new();
    let builds: Vec<&String> = names
        .iter()
        .filter(|n| n.starts_with(&format!("{prefix}-")) && n.ends_with(".ppn"))
        .collect();
    for name in &builds {
        let rest = &name[prefix.len() + 1..name.len() - 4];
        let mut readers = rules.readers.clone();
        readers.sort_by_key(|r| std::cmp::Reverse(r.len()));
        let Some(device) = readers.into_iter().find(|d| {
            rest == d
                || rest
                    .strip_prefix(d.as_str())
                    .is_some_and(|r| r.starts_with('-'))
        }) else {
            problems.push(Problem::error(
                "upload",
                name.to_string(),
                format!(
                    "names no reader: call it {prefix}-<device>.ppn with one of {}",
                    rules.readers.join(", ")
                ),
            ));
            continue;
        };
        if devices.contains(&device) {
            problems.push(Problem::error(
                "upload",
                name.to_string(),
                format!("a second build for the {device}: one build for each reader"),
            ));
        }
        devices.push(device.clone());
        let bytes = std::fs::read(dir.join(name)).unwrap_or_default();
        if bytes.len() as u64 > rules.max_build {
            problems.push(Problem::error(
                "upload",
                name.to_string(),
                format!("larger than {} MB", rules.max_build >> 20),
            ));
            continue;
        }
        let (header, signature) = match build(&bytes) {
            Ok(read) => read,
            Err(problem) => {
                problems.push(Problem::error("build", name.to_string(), problem));
                continue;
            }
        };
        if Some(header.target) != chip(&device) {
            problems.push(Problem::error(
                "build",
                name.to_string(),
                format!(
                    "built for the {:?}, which is not the {device}'s chip: give it the name of \
                     the reader it was built for",
                    header.target
                ),
            ));
        }
        let built_for = header.abi_major;
        if let Some(interface) = interface
            && built_for != interface
        {
            problems.push(Problem::error(
                "build",
                name.to_string(),
                format!(
                    "built for plugin interface {built_for}, and the PaperPaw it is for has \
                     {interface}: build it with that release's SDK and compilers"
                ),
            ));
        }
        match signature {
            Some(signature) => keys.push(signature.key),
            None => problems.push(Problem::error(
                "signature",
                name.to_string(),
                "not signed: sign it with your author key (`paperpaw-sign pack <elf> <out> --key \
                 <file>`, in the workflow from the secret PAPERPAW_PLUGIN_KEY)",
            )),
        }
    }
    keys.dedup();
    if keys.len() > 1 {
        problems.push(Problem::error(
            "signature",
            dir.display().to_string(),
            "the builds are signed by different keys: sign all with your one author key",
        ));
    }
    builds.len()
}

/// The folder's ZIP: `plugin.ini` at its top or in one folder at its top,
/// and `store/store.ini` beside it.
fn zip_check(dir: &Path, package: &str, plugin: &Plugin) -> Vec<Problem> {
    let mut problems = Vec::new();
    let storage = match paperpaw_fs::DirStorage::new(dir) {
        Ok(storage) => storage,
        Err(e) => return vec![Problem::error("upload", package, e.to_string())],
    };
    let zip = match paperpaw_zip::ZipArchive::open(&storage, package) {
        Ok(zip) => zip,
        Err(e) => {
            return vec![Problem::error(
                "upload",
                package,
                format!("not a ZIP the site reads: {e}"),
            )];
        }
    };
    let names: Vec<String> = zip
        .entries(&storage)
        .filter_map(Result::ok)
        .map(|e| e.name)
        .collect();
    let top = if names.iter().any(|n| n == "plugin.ini") {
        Some(String::new())
    } else {
        let folders: std::collections::BTreeSet<&str> = names
            .iter()
            .filter_map(|n| n.split_once('/').map(|(f, _)| f))
            .collect();
        (folders.len() == 1)
            .then(|| folders.into_iter().next().map(|f| format!("{f}/")))
            .flatten()
            .filter(|f| names.contains(&format!("{f}plugin.ini")))
    };
    match top {
        None => problems.push(Problem::error(
            "manifest",
            package,
            "no plugin.ini at its top (or in one folder at its top): zip the plugin's folder itself",
        )),
        Some(top) if !names.contains(&format!("{top}store/store.ini")) => {
            problems.push(Problem::error(
                "metadata",
                package,
                format!(
                    "no store/store.ini in it: zip the folder with store/ ({} has it?)",
                    plugin.folder.display()
                ),
            ))
        }
        Some(_) => {}
    }
    problems
}
