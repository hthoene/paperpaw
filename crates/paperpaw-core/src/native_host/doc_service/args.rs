//! Input rules of `core.document` (`docs/abi/document.md` 2): record order,
//! values and bit sets, checked before the host does anything.
use super::super::registrations::{self as wire, Record};
use paperpaw_native::host_glue::{ERR_DENIED, ERR_INVALID};

/// `doc_open.flags`: publisher styles, colour, background.
pub(super) const OPEN_FLAGS: u32 = 0b111;
/// The most `doc_option` rows of one open.
pub(super) const MAX_OPTIONS: usize = 12;
/// Default longest side of a page picture (interface 6 `IMAGE_FIT`).
pub(super) const IMAGE_FIT: u32 = 1_600;
/// The largest picture an `image` call may ask for.
pub(super) const IMAGE_SIDE_MAX: u32 = 1_600;

/// A parsed `open` input: the open record and how many options follow it.
pub(super) struct Open<'a> {
    pub(super) open: wire::ServiceDocOpen<'a>,
    pub(super) options: usize,
}

/// Exactly one `doc_open`, first, then at most 12 `doc_option`s. A missing,
/// repeated or out-of-order record is `ERR_INVALID`.
pub(super) fn parse_open(input: &[u8]) -> Result<Open<'_>, i32> {
    let mut open = None;
    let mut options = 0;
    let rows = wire::host_records_visit(input, MAX_OPTIONS + 1, &mut |record| {
        match (record, open.is_some()) {
            (Record::ServiceDocOpen(first), false) => open = Some(first),
            (Record::ServiceDocOption(option), true) => {
                check_option(&option)?;
                options += 1;
            }
            _ => return Err(ERR_INVALID),
        }
        Ok(())
    })?;
    // The count is the walk's own: nothing but the open record may be missing.
    debug_assert_eq!(rows, options + usize::from(open.is_some()));
    Ok(Open {
        open: open.ok_or(ERR_INVALID)?,
        options,
    })
}

/// Visit the `doc_option` rows of an already parsed `open` input, in order.
pub(super) fn each_option<'a>(input: &'a [u8], mut visit: impl FnMut(&wire::ServiceDocOption<'a>)) {
    // The input was walked and accepted by `parse_open`; a failure here is a
    // different batch and visits nothing more.
    let _ = wire::host_records_visit(input, MAX_OPTIONS + 1, &mut |record| {
        if let Record::ServiceDocOption(option) = record {
            visit(&option);
        }
        Ok(())
    });
}

/// An option's key is a full setting id (`<plugin>.<name>`).
fn check_option(option: &wire::ServiceDocOption<'_>) -> Result<(), i32> {
    if id_valid(option.key) {
        Ok(())
    } else {
        Err(ERR_INVALID)
    }
}

/// `<plugin>.<name>` ids: two or more dot-separated segments of 1..=32 bytes
/// `[a-z][a-z0-9-]*` (the service and setting key grammar).
pub(super) fn id_valid(key: &str) -> bool {
    let mut segments = 0;
    key.split('.').all(|part| {
        segments += 1;
        (1..=32).contains(&part.len())
            && part.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }) && segments >= 2
}

/// A file path on root 0 or 1: relative, the storage grammar, 1..=512 bytes.
pub(super) fn path_valid(path: &str) -> bool {
    path.len() <= 512 && paperpaw_platform::storage::is_valid_path(path)
}

/// `doc_open.root` of the file a hand-over names (`document.md` 1, draft 25).
pub(crate) const ROOT_HANDED: u32 = 5;

/// Whether a path of `root` may be named: root 1 never reaches the kernel's
/// and the plugins' folders and `paperpaw.ini` (the first path segment, as the
/// FAT driver compares names, `owner_paths::protected_first`); a book or a CSS
/// file there is `ERR_INVALID`, as for the file service (`files.md` 3). The
/// refusal comes before the access check, so a plugin without `uses.storage`
/// gets `ERR_INVALID` here too. A `~` short-name alias is not checked: the
/// card's driver (`paperpaw-fat`) names an entry by its long name only, so no
/// path reaches a protected folder by another name.
pub(super) fn reachable(root: u32, path: &str) -> bool {
    root != 1 || !super::super::owner_paths::protected_first(path.split('/').next().unwrap_or(""))
}

/// Values of `open` that need no host state. `long_side` is the screen's
/// long side in pixels.
pub(super) fn check_open(open: &wire::ServiceDocOpen<'_>, long_side: u32) -> Result<(), i32> {
    // The handed file (root 5) is for the book alone; a CSS file there is
    // denied (`document.md` 1).
    if open.css_root == ROOT_HANDED {
        return Err(ERR_DENIED);
    }
    if (open.root > 1 && open.root != ROOT_HANDED)
        || open.css_root > 1
        || open.flags & !OPEN_FLAGS != 0
        || !path_valid(open.path)
        || !reachable(open.root, open.path)
        || !(open.css_path.is_empty()
            || (path_valid(open.css_path) && reachable(open.css_root, open.css_path)))
        || !(open.format.is_empty() || id_valid(open.format))
        || open.image_side > IMAGE_SIDE_MAX
        || open.cover_side > long_side
    {
        return Err(ERR_INVALID);
    }
    Ok(())
}

/// The cover side when `open` asked for 0: the larger of 300 and 3/8 of the
/// screen's long side (interface 6 `cover_fit`).
pub(super) fn default_cover_side(long_side: u32) -> u32 {
    let three_eighths = u64::from(long_side) * 3 / 8;
    300.max(u32::try_from(three_eighths).unwrap_or(u32::MAX))
}

/// A page request: a non-zero cursor needs a generation, as REG pages.
pub(super) fn page_state(cursor: u32, generation: u32) -> Result<(), i32> {
    if cursor != 0 && generation == 0 {
        Err(paperpaw_native::host_glue::ERR_STATE)
    } else {
        Ok(())
    }
}

/// `image` side: 0 for the page's size, else at most 1,600: a larger one is
/// clamped, not refused (`document.md` 5, as `image_prepare`).
pub(super) fn image_side(asked: u32, page: u32) -> u32 {
    match asked {
        0 => page,
        _ => asked.min(IMAGE_SIDE_MAX),
    }
}
