//! `pack-source`, `sign-source` and `check-source`: a community plugin
//! source (#214, #318), a plugin library of one's own that readers add by its
//! address. The work is `paperpaw_publish::source`'s; the key is the one
//! `key` makes, which also signs the builds of the source's plugins.

use std::path::Path;

use paperpaw_publish::source::{self, About};

/// The key as the source's publisher tells it to readers.
fn say_key(key: &[u8; 32]) {
    println!("public key:  {}", paperpaw_digest::hex::encode(key));
    println!("fingerprint: {}", source::fingerprint(key));
}

/// `pack-source <plugins-dir> <out-dir> [--author <name>] [--date <day>]
/// [--key <secret-key-file>]`.
pub fn pack(input: &str, out: &str, mut rest: &[&str]) -> Result<(), String> {
    let (mut about, mut key_file) = (About::default(), None);
    while let [flag, value, tail @ ..] = rest {
        match *flag {
            "--author" => about.author = value,
            "--date" => about.date = value,
            "--key" => key_file = Some(*value),
            other => return Err(format!("pack-source: unknown option {other}")),
        }
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(
            "usage: pack-source <plugins-dir> <out-dir> [--author <name>] [--date <yyyy-mm-dd>] \
             [--key <secret-key-file>]"
                .into(),
        );
    }
    let seed = crate::secret_key(key_file)?;
    let packed = source::pack(Path::new(input), Path::new(out), &seed, about)?;
    for plugin in &packed.plugins {
        let builds = if plugin.devices.is_empty() {
            "no code".to_string()
        } else {
            format!("code for {}", plugin.devices.join(", "))
        };
        println!(
            "{} {}: {} files, {builds}",
            plugin.id, plugin.version, plugin.files
        );
    }
    println!("{out}/plugins.json: signed");
    say_key(&packed.key);
    Ok(())
}

/// `sign-source <plugins.json> [--key <secret-key-file>]`.
pub fn sign(list: &str, key_file: Option<&str>) -> Result<(), String> {
    let seed = crate::secret_key(key_file)?;
    let key = source::sign(Path::new(list), &seed)?;
    println!("{list}.sig: written");
    say_key(&key);
    Ok(())
}

/// `check-source <plugins.json>`.
pub fn check(list: &str) -> Result<(), String> {
    let key = source::check(Path::new(list))?;
    say_key(&key);
    println!("signature:   holds");
    Ok(())
}
