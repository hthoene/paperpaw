//! The inactive ABI 7 host implementation has its own reviewed snapshots.
//! Every line remains checked: this partition never blesses ABI 7 files.

const NAMESPACE: &str = "paperpaw_native::host_glue";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Owner {
    Exclusive,
    Shared,
}

struct Rule<'a> {
    path: &'a str,
    owner: Owner,
    descendants: bool,
}

struct Ownership<'a>(Vec<Rule<'a>>);

impl<'a> Ownership<'a> {
    fn parse(text: &'a str) -> Result<Self, String> {
        let mut out = Self(Vec::new());
        for line in text.lines() {
            let line = line.split('#').next().unwrap_or_default().trim();
            if line.is_empty() {
                continue;
            }
            let words: Vec<_> = line.split_whitespace().collect();
            let [kind, path] = words.as_slice() else {
                return Err(format!("invalid ABI ownership record: {line}"));
            };
            let (owner, descendants) = match *kind {
                "exclusive-type" => (Owner::Exclusive, true),
                "exclusive-item" => (Owner::Exclusive, false),
                "shared-type" => (Owner::Shared, true),
                _ => return Err(format!("unknown ABI ownership kind: {line}")),
            };
            if ((!in_namespace(path) && owner != Owner::Shared)
                || !path.starts_with("paperpaw_native::"))
                || (descendants && *path == NAMESPACE)
                || path.ends_with(':')
                || !path
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b':')
            {
                return Err(format!("invalid ABI ownership path: {line}"));
            }
            let rule = Rule {
                path,
                owner,
                descendants,
            };
            if out
                .0
                .iter()
                .any(|old| covers(old, path) || covers(&rule, old.path))
            {
                return Err(format!("overlapping ABI ownership record: {line}"));
            }
            out.0.push(rule);
        }
        Ok(out)
    }

    fn owner(&self, path: &str) -> Option<Owner> {
        self.0
            .iter()
            .find(|rule| covers(rule, path))
            .map(|rule| rule.owner)
    }
}

fn covers(rule: &Rule<'_>, path: &str) -> bool {
    path == rule.path
        || (rule.descendants
            && path
                .strip_prefix(rule.path)
                .is_some_and(|rest| rest.starts_with("::")))
}

#[derive(Debug, Default, Eq, PartialEq)]
struct Partition {
    legacy: String,
    abi7: String,
}

/// Filter both generations for comparison, preserving the original baseline
/// unconditionally until the coordinated raise. Check ABI 7 first, even
/// when `PAPERPAW_BLESS` is set. The independent ABI lock gate still applies.
pub(super) fn legacy(
    crate_: &str,
    kind: &str,
    kept: &str,
    current: &str,
) -> (String, String, String) {
    let folder = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../abi/interface-7");
    let ownership = std::fs::read_to_string(folder.join("ownership.txt"))
        .unwrap_or_else(|error| panic!("required ABI 7 ownership manifest: {error}"));
    let snapshot = (crate_ == "paperpaw-native").then(|| {
        let path = folder.join(format!("paperpaw-native.{kind}"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("required ABI 7 snapshot {}: {error}", path.display()))
            .replace("\r\n", "\n")
    });
    let (before, after) = checked_legacy(kept, current, snapshot.as_deref(), &ownership)
        .unwrap_or_else(|error| panic!("{crate_}.{kind}: {error}"));
    let write = baseline_text(kept, &before, &after);
    (before, after, write)
}

fn baseline_text(original: &str, _before: &str, _after: &str) -> String {
    // This temporary partition never advances the frozen ABI 6 reference,
    // even for normally additive changes. Retire it at the coordinated raise.
    original.to_string()
}

pub(super) fn checked_legacy(
    kept: &str,
    current: &str,
    snapshot: Option<&str>,
    ownership: &str,
) -> Result<(String, String), String> {
    let ownership = Ownership::parse(ownership)?;
    let kept = partition(kept, &ownership)?;
    let current = partition(current, &ownership)?;
    match snapshot {
        Some(snapshot) if current.abi7 != snapshot => {
            return Err(
                "ABI 7 differs from its reviewed snapshot; review and update \
                abi/interface-7 explicitly, PAPERPAW_BLESS cannot update it"
                    .to_string(),
            );
        }
        None if !current.abi7.is_empty() || !kept.abi7.is_empty() => {
            return Err("ABI 7 definitions outside paperpaw-native have no snapshot".to_string());
        }
        _ => {}
    }
    Ok((kept.legacy, current.legacy))
}

fn partition(text: &str, ownership: &Ownership<'_>) -> Result<Partition, String> {
    let mut out = Partition::default();
    for line in text.split_inclusive('\n') {
        // Comments and blank lines stay with the legacy file, byte for byte.
        if (!line.trim_start().starts_with('#') || line.trim_start().starts_with("#["))
            && namespace_paths(line).next().is_some()
        {
            let path = defining_owner(line).unwrap_or_default();
            let owner = ownership.owner(path);
            if in_namespace(path) && owner.is_none() {
                return Err(format!("unclassified ABI namespace definition: {line}"));
            }
            for reference in namespace_paths(line) {
                let referred = ownership
                    .owner(reference)
                    .ok_or_else(|| format!("unclassified ABI namespace reference: {line}"))?;
                if owner != Some(Owner::Exclusive) && referred == Owner::Exclusive {
                    return Err(format!(
                        "legacy definition references exclusive ABI 7: {line}"
                    ));
                }
            }
            if owner.is_some() {
                out.abi7.push_str(line);
            }
            if owner == Some(Owner::Exclusive) {
                continue;
            }
        }
        out.legacy.push_str(line);
    }
    Ok(out)
}

fn in_namespace(path: &str) -> bool {
    path == NAMESPACE
        || path
            .strip_prefix(NAMESPACE)
            .is_some_and(|rest| rest.starts_with("::"))
}

fn namespace_paths(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
        .map(|path| path.trim_end_matches(':'))
        .filter(|path| in_namespace(path))
}

/// API declarations use their published path; shapes use their defining
/// path, including private generated types. An impl belongs to its self
/// type, never to a trait mentioned before `for` or a generic argument.
fn defining_owner(mut line: &str) -> Option<&str> {
    line = line.trim();
    while let Some(attribute) = line.strip_prefix("#[") {
        line = attribute.split_once("] ")?.1.trim_start();
    }
    if line.starts_with("pub use ")
        && let Some((_, alias)) = line.split_once(" as ")
    {
        return Some(path_head(alias));
    }
    if let Some(rest) = line.strip_prefix("impl")
        && (rest.starts_with('<') || rest.starts_with(' '))
    {
        return impl_owner(rest.trim_start());
    }
    for token in line.split_whitespace() {
        if matches!(
            token,
            "pub"
                | "const"
                | "unsafe"
                | "async"
                | "extern"
                | "c"
                | "fn"
                | "mod"
                | "struct"
                | "enum"
                | "union"
                | "trait"
                | "type"
                | "static"
                | "mut"
                | "use"
                | "macro"
        ) || token.starts_with('"')
        {
            continue;
        }
        return Some(path_head(token));
    }
    None
}

fn path_head(text: &str) -> &str {
    let end = text
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
        .unwrap_or(text.len());
    text[..end].trim_end_matches(':')
}

fn impl_owner(mut text: &str) -> Option<&str> {
    if text.starts_with('<') {
        let mut depth = 0usize;
        let end = text.char_indices().find_map(|(i, c)| {
            if c == '<' {
                depth += 1;
            } else if c == '>' && !text[..i].ends_with('-') {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i + 1);
                }
            }
            None
        })?;
        text = text[end..].trim_start();
    }
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        if depth == 0 && text[i..].starts_with(" for ") {
            return Some(path_head(text[i + 5..].trim_start()));
        }
        match c {
            '<' | '(' | '[' => depth += 1,
            '>' if text[..i].ends_with('-') => {}
            '>' | ')' | ']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    Some(path_head(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEGACY: &str = "pub mod paperpaw_native\n";
    const ABI7: &str = "pub mod paperpaw_native::host_glue\n";
    const OWNERS: &str = "\
exclusive-item paperpaw_native::host_glue
exclusive-item paperpaw_native::host_glue::buffers
exclusive-item paperpaw_native::host_glue::MINOR
exclusive-item paperpaw_native::host_glue::ERR_IO
exclusive-item paperpaw_native::host_glue::SETTINGS_LAYOUT
exclusive-item paperpaw_native::host_glue::generated::SETTINGS_LAYOUT
exclusive-type paperpaw_native::host_glue::PpHost
exclusive-type paperpaw_native::host_glue::PpPlugin
exclusive-type paperpaw_native::host_glue::Services
exclusive-type paperpaw_native::host_glue::ShellCtx
exclusive-type paperpaw_native::host_glue::SettingValueRef
exclusive-type paperpaw_native::host_glue::generated::PpHost
exclusive-type paperpaw_native::host_glue::generated::SettingValueRef
";

    fn split(text: &str) -> Result<Partition, String> {
        partition(text, &Ownership::parse(OWNERS).unwrap())
    }

    fn shared_owners() -> String {
        OWNERS.replace(
            "exclusive-type paperpaw_native::host_glue::PpPlugin",
            "shared-type paperpaw_native::host_glue::PpPlugin",
        )
    }

    #[test]
    fn appended_abi7_shape_refuses_a_stale_snapshot() {
        let old = "struct paperpaw_native::host_glue::generated::PpHost { size: u32 }\n";
        let new = old.replace("size: u32", "size: u32, extra: u32");
        assert!(checked_legacy(old, &new, Some(old), OWNERS).is_err());
        assert!(checked_legacy(old, &new, Some(&new), OWNERS).is_ok());
        assert!(checked_legacy(old, &new, None, OWNERS).is_err());
    }

    #[test]
    fn unchanged_legacy_keeps_its_physical_baseline_on_bless() {
        let kept = format!("{LEGACY}{ABI7}");
        let new = format!("{kept}pub const paperpaw_native::host_glue::MINOR: u16\n");
        let snapshot = split(&new).unwrap().abi7;
        let (before, after) = checked_legacy(&kept, &new, Some(&snapshot), OWNERS).unwrap();
        assert_eq!(before, LEGACY);
        assert_eq!(after, LEGACY);
        assert_eq!(baseline_text(&kept, &before, &after), kept);
        assert!(checked_legacy(&kept, &new, Some(ABI7), OWNERS).is_err());
        assert_eq!(baseline_text(&kept, &before, "changed6\n"), kept);
    }

    #[test]
    fn additive_legacy_changes_cannot_strip_or_advance_the_frozen_baseline() {
        let kept = format!("{LEGACY}{ABI7}");
        let added = "pub fn paperpaw_native::new_legacy_function()\n";
        let current = format!("{kept}{added}");
        let (before, after) = checked_legacy(&kept, &current, Some(ABI7), OWNERS).unwrap();
        assert_eq!(before, LEGACY);
        assert_eq!(after, format!("{LEGACY}{added}"));
        let first_bless = baseline_text(&kept, &before, &after);
        assert_eq!(first_bless, kept);
        let (before, after) = checked_legacy(&first_bless, &current, Some(ABI7), OWNERS).unwrap();
        assert_eq!(baseline_text(&first_bless, &before, &after), kept);
    }

    #[test]
    fn matching_abi7_snapshot_preserves_changed_legacy_entry_and_api() {
        for name in ["Entry", "Api"] {
            let kept = format!("struct paperpaw_native::interface::{name} {{ size: u32 }}\n");
            let changed = kept.replace("u32", "u64");
            let (before, after) = checked_legacy(
                &format!("{kept}{ABI7}"),
                &format!("{changed}{ABI7}"),
                Some(ABI7),
                OWNERS,
            )
            .unwrap();
            assert_eq!(before, kept);
            assert_eq!(after, changed);
            assert_ne!(before, after);
        }
    }

    #[test]
    fn legacy_references_to_exclusive_abi7_are_rejected() {
        for line in [
            "pub fn paperpaw_native::Entry::fingerprint_at(*const paperpaw_native::host_glue::PpHost)",
            "struct paperpaw_native::interface::Entry { host: paperpaw_native::host_glue::PpHost }",
            "impl core::convert::From<paperpaw_native::host_glue::PpHost> for paperpaw_native::Api",
            "impl paperpaw_native::host_glue::Services for paperpaw_native::Api",
            "impl<T: paperpaw_native::host_glue::Services> paperpaw_native::Api<T>",
            "pub use paperpaw_native::host_glue::PpHost as paperpaw_native::Api",
        ] {
            assert!(split(line).is_err(), "{line}");
        }
    }

    #[test]
    fn reviewed_public_reexports_and_private_generated_types_are_abi7() {
        for line in [
            ABI7.trim(),
            "#[non_exhaustive] pub enum paperpaw_native::host_glue::SettingValueRef<'a>",
            "pub const paperpaw_native::host_glue::SETTINGS_LAYOUT: u32",
            "pub const paperpaw_native::host_glue::ERR_IO: i32",
            "pub use paperpaw_native::host_glue::SettingValueRef",
            "enum paperpaw_native::host_glue::generated::SettingValueRef<'a> { Absent }",
            "const paperpaw_native::host_glue::generated::SETTINGS_LAYOUT: u32 = 1u32",
            "impl core::fmt::Debug for paperpaw_native::host_glue::PpPlugin",
            "impl<'a> paperpaw_native::host_glue::ShellCtx<'a>",
            "impl<T: core::convert::Into<u8>> core::convert::From<T> for paperpaw_native::host_glue::PpPlugin",
        ] {
            let found = split(line).unwrap();
            assert_eq!(found.abi7, line);
            assert_eq!(found.legacy, "");
        }
    }

    #[test]
    fn unreviewed_namespace_definitions_and_references_fail() {
        let owners = OWNERS.replace("exclusive-item paperpaw_native::host_glue::ERR_IO\n", "");
        let owners = Ownership::parse(&owners).unwrap();
        for line in [
            "pub const paperpaw_native::host_glue::ERR_IO: i32",
            "pub const paperpaw_native::host_glue::MINOR: i32 = paperpaw_native::host_glue::ERR_IO",
        ] {
            assert!(partition(line, &owners).is_err(), "{line}");
        }
        let item = Ownership::parse("exclusive-item paperpaw_native::host_glue::PpPlugin").unwrap();
        assert!(partition("pub paperpaw_native::host_glue::PpPlugin::size: u32", &item).is_err());
    }

    #[test]
    fn shared_types_and_members_stay_in_both_snapshots() {
        let owners = shared_owners();
        let ownership = Ownership::parse(&owners).unwrap();
        for line in [
            "struct paperpaw_native::host_glue::PpPlugin { size: u32 }\n",
            "pub paperpaw_native::host_glue::PpPlugin::size: u32\n",
            "impl core::fmt::Debug for paperpaw_native::host_glue::PpPlugin\n",
        ] {
            let found = partition(line, &ownership).unwrap();
            assert_eq!(found.abi7, line);
            assert_eq!(found.legacy, line);
            let changed = line.replace("u32", "u64");
            let (before, after) = checked_legacy(line, &changed, Some(&changed), &owners).unwrap();
            assert_eq!(before, line);
            assert_eq!(after, changed);
            assert_eq!(baseline_text(line, &before, &after), line);
        }
    }

    #[test]
    fn shared_legacy_references_are_allowed_but_exclusive_references_are_not() {
        let owners = shared_owners();
        let ownership = Ownership::parse(&owners).unwrap();
        let line = "struct paperpaw_native::interface::Entry { plugin: paperpaw_native::host_glue::PpPlugin }";
        let found = partition(line, &ownership).unwrap();
        assert_eq!(found.legacy, line);
        assert_eq!(found.abi7, "");
        let crossing = "struct paperpaw_native::host_glue::PpPlugin { host: paperpaw_native::host_glue::PpHost }";
        assert!(partition(crossing, &ownership).is_err());
    }

    #[test]
    fn manifest_rejects_ambiguous_rules_and_namespace_type_wildcards() {
        for owners in [
            "exclusive-type paperpaw_native::host_glue",
            "exclusive-type paperpaw_native::host_glue::*",
            "exclusive-type paperpaw_native::host_glue::PpPlugin\nshared-type paperpaw_native::host_glue::PpPlugin",
            "exclusive-type paperpaw_native::host_glue::PpPlugin\nexclusive-item paperpaw_native::host_glue::PpPlugin::size",
        ] {
            assert!(Ownership::parse(owners).is_err(), "{owners}");
        }
    }

    #[test]
    fn namespace_lookalikes_stay_in_legacy() {
        for line in [
            "pub mod paperpaw_native::host_glue_extra",
            "struct paperpaw_native::host_glue2::PpPlugin { size: u32 }",
            "pub fn paperpaw_native::Entry::fingerprint_at(other_paperpaw_native::host_glue::PpHost)",
            "pub fn other::paperpaw_native::host_glue::host_table()",
            "# A comment mentioning paperpaw_native::host_glue\n",
        ] {
            let found = split(line).unwrap();
            assert_eq!(found.legacy, line);
            assert_eq!(found.abi7, "");
        }
    }
}
