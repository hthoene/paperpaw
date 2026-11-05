//! The inactive ABI 7 host implementation has its own reviewed snapshots.
//! Every line remains checked: this partition never blesses ABI 7 files.

const NAMESPACE: &str = "paperpaw_native::host_glue";

#[derive(Debug, Default, Eq, PartialEq)]
struct Partition {
    legacy: String,
    abi7: String,
}

/// Filter both generations so historical ABI 7 entries can leave the ABI 6
/// baseline through an ordinary bless. Check the current ABI 7 first, even
/// when `PAPERPAW_BLESS` is set. The independent ABI lock gate still applies.
pub(super) fn legacy(crate_: &str, kind: &str, kept: &str, current: &str) -> (String, String) {
    let snapshot = (crate_ == "paperpaw-native").then(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../abi/interface-7")
            .join(format!("paperpaw-native.{kind}"));
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("required ABI 7 snapshot {}: {error}", path.display()))
            .replace("\r\n", "\n")
    });
    checked_legacy(kept, current, snapshot.as_deref())
        .unwrap_or_else(|error| panic!("{crate_}.{kind}: {error}"))
}

pub(super) fn checked_legacy(
    kept: &str,
    current: &str,
    snapshot: Option<&str>,
) -> Result<(String, String), String> {
    let kept = partition(kept)?;
    let current = partition(current)?;
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

fn partition(text: &str) -> Result<Partition, String> {
    let mut out = Partition::default();
    for line in text.split_inclusive('\n') {
        // Comments and blank lines stay with the legacy file, byte for byte.
        if (!line.trim_start().starts_with('#') || line.trim_start().starts_with("#["))
            && mentions_namespace(line)
        {
            let owner = defining_owner(line).unwrap_or_default();
            if !in_namespace(owner) {
                return Err(format!("legacy definition references ABI 7: {line}"));
            }
            out.abi7.push_str(line);
            continue;
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

fn mentions_namespace(line: &str) -> bool {
    line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != ':')
        .any(in_namespace)
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

    #[test]
    fn appended_abi7_shape_refuses_a_stale_snapshot() {
        let old = "struct paperpaw_native::host_glue::generated::PpHost { size: u32 }\n";
        let new = old.replace("size: u32", "size: u32, extra: u32");
        assert!(checked_legacy(old, &new, Some(old)).is_err());
        assert!(checked_legacy(old, &new, Some(&new)).is_ok());
        assert!(checked_legacy(old, &new, None).is_err());
    }

    #[test]
    fn historical_abi7_is_removed_only_after_current_snapshot_matches() {
        let kept = format!("{LEGACY}{ABI7}");
        let new = format!("{kept}pub const paperpaw_native::host_glue::MINOR: u16\n");
        let snapshot = partition(&new).unwrap().abi7;
        assert_eq!(
            checked_legacy(&kept, &new, Some(&snapshot)).unwrap(),
            (LEGACY.to_string(), LEGACY.to_string())
        );
        assert!(checked_legacy(&kept, &new, Some(ABI7)).is_err());
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
            )
            .unwrap();
            assert_eq!(before, kept);
            assert_eq!(after, changed);
            assert_ne!(before, after);
        }
    }

    #[test]
    fn legacy_references_to_abi7_are_rejected() {
        for line in [
            "pub fn paperpaw_native::old(paperpaw_native::host_glue::PpHost)",
            "struct paperpaw_sdk::Old { host: paperpaw_native::host_glue::PpHost }",
            "impl core::convert::From<paperpaw_native::host_glue::PpHost> for paperpaw_native::Api",
            "impl paperpaw_native::host_glue::Services for paperpaw_native::Api",
            "impl<T: paperpaw_native::host_glue::Services> paperpaw_native::Api<T>",
            "pub type paperpaw_native::Alias = paperpaw_native::host_glue::PpHost",
            "pub use paperpaw_native::host_glue::PpHost as paperpaw_native::Alias",
        ] {
            assert!(partition(line).is_err(), "{line}");
        }
    }

    #[test]
    fn public_reexports_and_private_generated_types_are_abi7() {
        for line in [
            ABI7.trim(),
            "#[repr(C)] pub struct paperpaw_native::host_glue::SettingValueRef<'a>",
            "pub const paperpaw_native::host_glue::SETTINGS_LAYOUT: u32",
            "pub use paperpaw_native::host_glue::SettingValueRef",
            "enum paperpaw_native::host_glue::generated::SettingValueRef<'a> { Absent }",
            "const paperpaw_native::host_glue::generated::SETTINGS_LAYOUT: u32 = 1u32",
            "impl core::fmt::Debug for paperpaw_native::host_glue::SettingValueRef<'_>",
            "impl<'a> paperpaw_native::host_glue::SettingValueRef<'a>",
            "impl<T: core::convert::Into<u8>> core::convert::From<T> for paperpaw_native::host_glue::Thing<T>",
        ] {
            let found = partition(line).unwrap();
            assert_eq!(found.abi7, line);
            assert_eq!(found.legacy, "");
        }
    }

    #[test]
    fn namespace_lookalikes_stay_in_legacy() {
        for line in [
            "pub mod paperpaw_native::host_glue_extra",
            "struct paperpaw_native::host_glue2::Thing { size: u32 }",
            "pub fn paperpaw_native::old(other_paperpaw_native::host_glue::PpHost)",
            "pub fn other::paperpaw_native::host_glue::function()",
            "# A comment mentioning paperpaw_native::host_glue\n",
        ] {
            let found = partition(line).unwrap();
            assert_eq!(found.legacy, line);
            assert_eq!(found.abi7, "");
        }
    }
}
