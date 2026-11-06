//! Writing a package from the interface-6 plugin's sources.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::packages::{FORMATS, PACKAGES, Package, VERSION};

fn read(path: &Path) -> io::Result<String> {
    fs::read_to_string(path)
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

fn put(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))
}

/// The value of `key` in the `[messages]` of an English catalog.
fn message(catalog: &str, key: &str) -> Option<String> {
    catalog.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().to_string())
    })
}

/// Its `plugin.ini`: name and description are the English ones of its catalog.
fn plugin_ini(package: &Package, english: &str) -> io::Result<String> {
    let field = |what: &str| {
        let key = format!("{}.plugin.{what}", package.id);
        message(english, &key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: no {key}", package.id),
            )
        })
    };
    let mut ini = format!(
        "[plugin]\nid = {}\nname = {}\ndescription = {}\nversion = {VERSION}\nformats = {FORMATS}\n\n[hyphenation]\n",
        package.id,
        field("name")?,
        field("description")?,
    );
    for &(tag, stem) in package.languages {
        ini.push_str(&format!("{tag} = hyphenation/{stem}.pphp\n"));
    }
    Ok(ini)
}

/// Writes `package` as `<out>/<id>/`, from the sources under `<root>/plugins/<id>/`.
pub fn build(root: &Path, out: &Path, package: &Package) -> io::Result<PathBuf> {
    let source = root.join("plugins").join(package.id);
    let dir = out.join(package.id);
    let patterns = source.join("patterns");
    let exceptions = match package.exceptions {
        Some(name) => read(&patterns.join(name))?,
        None => String::new(),
    };
    let (left, right) = package.minimums;
    let table = paperpaw_hyphen::compile(
        &read(&patterns.join(package.patterns))?,
        &exceptions,
        left,
        right,
    );
    for stem in package.tables() {
        put(&dir.join(format!("hyphenation/{stem}.pphp")), &table)?;
    }
    put(
        &dir.join("hyphenation/README.md"),
        read(&patterns.join("README.md"))?.as_bytes(),
    )?;
    let english = read(&source.join("i18n/en.ini"))?;
    put(
        &dir.join("plugin.ini"),
        plugin_ini(package, &english)?.as_bytes(),
    )?;
    for file in ["i18n/en.ini", "i18n/de.ini", "how/en.txt", "how/de.txt"] {
        put(&dir.join(file), read(&source.join(file))?.as_bytes())?;
    }
    Ok(dir)
}

/// Writes all thirteen; the folders written.
pub fn build_all(root: &Path, out: &Path) -> io::Result<Vec<PathBuf>> {
    PACKAGES.iter().map(|p| build(root, out, p)).collect()
}
