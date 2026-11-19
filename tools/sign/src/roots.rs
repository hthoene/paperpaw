//! The lists of root certificates for the X4's TLS (#387, `paperpaw_x4::tls_roots`),
//! made from Mozilla's roots as `webpki-roots` has them:
//!
//! - `roots-pack builtin <list.txt> <out.bin>`: the roots `list.txt` names
//!   (one a line, its label, then ` # ` and why it is in), as the file the
//!   firmware has built in (`shells/xteink/roots-builtin.bin`, no signature);
//! - `roots-pack all <out.pack> [--key <file>]`: all of Mozilla's roots, as
//!   the data plugin `tls-roots` holds them, signed with the plugin key in the
//!   file (without `--key`, not signed: a device does not read such a file).
//!
//! A root's label is the values of its name in order, ` / ` between them
//! (`ISRG Root X1 / Internet Security Research Group / US`...), as `list`
//! prints them.

use paperpaw_x4::tls_roots;
use webpki_roots::TLS_SERVER_ROOTS;

/// The next DER item of `der`: its tag, its contents and what follows.
fn item(der: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = der.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = if first < 0x80 {
        (usize::from(first), rest)
    } else {
        let bytes = usize::from(first & 0x7f);
        if bytes == 0 || bytes > 3 || rest.len() < bytes {
            return None;
        }
        let (len, rest) = rest.split_at(bytes);
        (len.iter().fold(0, |n, &b| n << 8 | usize::from(b)), rest)
    };
    (rest.len() >= len).then(|| (tag, &rest[..len], &rest[len..]))
}

/// The values of a name (a subject without its `SEQUENCE` header) in order.
fn label(subject: &[u8]) -> String {
    let mut values = Vec::new();
    let mut rest = subject;
    while let Some((0x31, set, after)) = item(rest) {
        rest = after;
        let mut set = set;
        while let Some((0x30, pair, next)) = item(set) {
            set = next;
            if let Some((0x06, _, value)) = item(pair)
                && let Some((_, text, _)) = item(value)
            {
                values.push(String::from_utf8_lossy(text).into_owned());
            }
        }
    }
    values.reverse();
    values.join(" / ")
}

/// The roots as `(label, name, key)`.
fn roots() -> Vec<(String, &'static [u8], &'static [u8])> {
    TLS_SERVER_ROOTS
        .iter()
        .map(|root| {
            (
                label(root.subject.as_ref()),
                root.subject.as_ref(),
                root.subject_public_key_info.as_ref(),
            )
        })
        .collect()
}

pub fn run(args: &[&str]) -> Result<(), String> {
    match args {
        ["list"] => {
            for (label, name, key) in roots() {
                println!("{label} # {} + {} bytes", name.len(), key.len());
            }
            Ok(())
        }
        ["builtin", list, out] => builtin(list, out),
        ["all", out] => all(out, None),
        ["all", out, "--key", key] => all(out, Some(key)),
        _ => Err(
            "usage: roots-pack list | builtin <list.txt> <out.bin> | all <out.pack> [--key <file>]"
                .into(),
        ),
    }
}

/// The labels of a list file: each line up to ` # `; blank lines and lines
/// that start with `#` are left out.
fn labels(text: &str) -> Vec<&str> {
    text.lines()
        .map(|line| line.split(" # ").next().unwrap_or("").trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn builtin(list: &str, out: &str) -> Result<(), String> {
    let text = std::fs::read_to_string(list).map_err(|e| format!("{list}: {e}"))?;
    let all = roots();
    let mut chosen = Vec::new();
    for wanted in labels(&text) {
        let found: Vec<_> = all.iter().filter(|(label, ..)| label == wanted).collect();
        match found.as_slice() {
            [one] => chosen.push((one.1, one.2)),
            [] => return Err(format!("{list}: no root is labelled {wanted:?}")),
            _ => return Err(format!("{list}: more than one root is labelled {wanted:?}")),
        }
    }
    let file = tls_roots::build(&chosen);
    std::fs::write(out, &file).map_err(|e| format!("{out}: {e}"))?;
    println!("{out}: {} roots, {} bytes", chosen.len(), file.len());
    Ok(())
}

fn all(out: &str, key_file: Option<&str>) -> Result<(), String> {
    let roots: Vec<(&[u8], &[u8])> = roots().into_iter().map(|(_, n, k)| (n, k)).collect();
    let mut file = tls_roots::build(&roots);
    let signed = key_file.is_some();
    if let Some(key_file) = key_file {
        let seed = crate::secret_key(Some(key_file))?;
        file = tls_roots::sign(&file, &seed);
    }
    std::fs::write(out, &file).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{out}: {} roots, {} bytes{}",
        roots.len(),
        file.len(),
        if signed { ", signed" } else { ", NOT signed" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The list the firmware has built in names roots Mozilla has, and the
    /// committed file is what the tool makes of it.
    #[test]
    fn the_built_in_file_is_made_from_its_list() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let list = std::fs::read_to_string(format!("{root}/tools/roots/builtin.txt")).unwrap();
        let out = std::env::temp_dir().join("paperpaw-roots-builtin-test.bin");
        builtin(
            &format!("{root}/tools/roots/builtin.txt"),
            out.to_str().unwrap(),
        )
        .unwrap();
        let made = std::fs::read(&out).unwrap();
        let committed = std::fs::read(format!("{root}/shells/xteink/roots-builtin.bin")).unwrap();
        assert!(!labels(&list).is_empty());
        assert_eq!(
            made, committed,
            "regenerate: target/release/paperpaw-sign roots-pack builtin tools/roots/builtin.txt shells/xteink/roots-builtin.bin"
        );
    }
}
