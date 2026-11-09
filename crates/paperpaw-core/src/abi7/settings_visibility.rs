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
    visibility_of(caller.as_str(), grant, secret, editor)
}

/// [`visibility_for`] for a caller known by its id text (REG rows).
pub(crate) fn visibility_of(
    caller: &str,
    grant: bool,
    secret: bool,
    editor: Option<&str>,
) -> Visibility {
    if grant && editor != Some(caller) {
        Visibility::Hidden
    } else if secret {
        Visibility::Redacted
    } else {
        Visibility::Readable
    }
}

/// A canonical identity as two borrowed spans, with at most one placement.
/// Comparing spans avoids both heap allocation and a large stack scratch buffer.
pub(crate) struct SettingIdentity<'a> {
    pub(crate) head: &'a str,
    tail: &'a str,
}
impl<'a> SettingIdentity<'a> {
    pub(crate) fn new(key: &'a str) -> Result<Self, i32> {
        use paperpaw_native::host_glue::ERR_INVALID;
        let mut placement = None;
        for segment in key.split('.') {
            let name = if let Some((name, number)) = segment.split_once('@') {
                if placement.is_some()
                    || number.is_empty()
                    || !number.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err(ERR_INVALID);
                }
                placement = key.find('@');
                name
            } else {
                segment
            };
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
            {
                return Err(ERR_INVALID);
            }
        }
        let Some(at) = placement else {
            return Ok(Self {
                head: key,
                tail: "",
            });
        };
        let end = key[at..].find('.').map_or(key.len(), |dot| at + dot);
        Ok(Self {
            head: &key[..at],
            tail: &key[end..],
        })
    }
    pub(crate) fn bytes(&self) -> impl Iterator<Item = u8> + '_ {
        self.head.bytes().chain(self.tail.bytes())
    }
    pub(crate) fn matches(&self, key: &str) -> bool {
        self.bytes().eq(key.bytes())
    }
    /// The definition the key names: a placed key (`plugin.widget@2.token`)
    /// names the definition of its plugin (`plugin.widget.token`). `None` for
    /// a key nothing defines. No allocation.
    pub(crate) fn definition<'s>(
        &self,
        settings: &'s paperpaw_sdk::Settings,
    ) -> Option<&'s paperpaw_sdk::SettingSpec> {
        settings.specs().find(|spec| self.matches(spec.id.as_str()))
    }
}
