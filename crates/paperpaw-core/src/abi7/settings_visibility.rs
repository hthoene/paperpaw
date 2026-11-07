//! Shared value visibility for generic slot-5 reads and REG rows.
use paperpaw_sdk::Id;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Visibility {
    Hidden,
    Redacted,
    Readable,
}

/// Id and editor come from host state. Neither is decoded from request bytes.
/// Secret values remain redacted here; only slot 5 has an own-secret exception.
pub(crate) fn visibility(caller: &Id, key: &str, secret: bool, editor: Option<&str>) -> Visibility {
    visibility_for(caller, key.starts_with("core.grant."), secret, editor)
}

pub(crate) fn visibility_for(
    caller: &Id,
    grant: bool,
    secret: bool,
    editor: Option<&str>,
) -> Visibility {
    if grant && editor != Some(caller.as_str()) {
        Visibility::Hidden
    } else if secret {
        Visibility::Redacted
    } else {
        Visibility::Readable
    }
}
