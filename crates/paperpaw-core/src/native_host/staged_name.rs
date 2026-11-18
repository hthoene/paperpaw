//! The host's one staged-file name for saved files (`core.net` saves and
//! `core.sockets` reads to a file): `<target>.pp-staged`, renamed over the
//! target only when the bytes are whole. A target that already ends in the
//! suffix (any case) is refused, so no plugin can name the host's temporary.

/// The suffix of a staged file.
pub(crate) const SUFFIX: &str = ".pp-staged";

/// Whether `path` ends in the staged suffix, in any case.
pub(crate) fn is_staged(path: &str) -> bool {
    let (path, suffix) = (path.as_bytes(), SUFFIX.as_bytes());
    path.len() >= suffix.len() && path[path.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
}

/// The staged name of `target`.
///
/// `None` when the allocation fails.
pub(crate) fn staged_path(target: &str) -> Option<alloc::string::String> {
    let mut name = alloc::string::String::new();
    name.try_reserve_exact(target.len() + SUFFIX.len()).ok()?;
    name.push_str(target);
    name.push_str(SUFFIX);
    Some(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_suffix_in_any_case_is_a_staged_name() {
        for yes in [
            "a.pp-staged",
            "a.PP-STAGED",
            "dir/b.Pp-Staged",
            ".pp-staged",
        ] {
            assert!(is_staged(yes), "{yes}");
        }
        for no in [
            "a.part",
            "a.pp-staged.txt",
            "pp-staged",
            "a.pp-stage",
            "",
            "é",
        ] {
            assert!(!is_staged(no), "{no}");
        }
    }

    #[test]
    fn the_staged_path_sits_beside_the_target() {
        assert_eq!(staged_path("dir/a.epub").unwrap(), "dir/a.epub.pp-staged");
    }
}
