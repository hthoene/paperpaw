//! The event tags an instance subscribed to, kept as a small bit set so that
//! "is this plugin subscribed to event E" is a read without allocation
//! (`core.net` asks it before it accepts a request or a server).
use paperpaw_native::host_glue::ERR_INVALID;

/// A set of event wire tags below 256.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventTags([u64; 4]);

impl EventTags {
    /// Tags from this value on cannot be held (the wire's event tags end well
    /// below it).
    #[cfg(test)]
    pub(crate) const LIMIT: u32 = 256;

    /// The tags below the limit for which `has` says yes.
    pub(crate) fn collect(mut has: impl FnMut(u32) -> bool) -> Self {
        let mut set = Self::default();
        for tag in 0..256 {
            if has(tag) {
                let _ = set.insert(tag);
            }
        }
        set
    }

    /// Add `tag`; `ERR_INVALID` for a tag beyond the set (nothing is added).
    pub(crate) fn insert(&mut self, tag: u32) -> Result<(), i32> {
        let word = self.0.get_mut((tag / 64) as usize).ok_or(ERR_INVALID)?;
        *word |= 1 << (tag % 64);
        Ok(())
    }

    /// Whether `tag` was added. A tag beyond the set was never added.
    pub(crate) fn contains(&self, tag: u32) -> bool {
        self.0
            .get((tag / 64) as usize)
            .is_some_and(|word| word & (1 << (tag % 64)) != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_host::registrations as wire;

    #[test]
    fn a_tag_is_in_the_set_once_added() {
        let mut tags = EventTags::default();
        assert!(!tags.contains(99));
        tags.insert(99).unwrap();
        tags.insert(0).unwrap();
        tags.insert(255).unwrap();
        assert!(tags.contains(99) && tags.contains(0) && tags.contains(255));
        assert!(!tags.contains(100) && !tags.contains(1) && !tags.contains(254));
    }

    #[test]
    fn a_tag_beyond_the_limit_is_refused_and_never_contained() {
        let mut tags = EventTags::default();
        assert_eq!(tags.insert(EventTags::LIMIT), Err(ERR_INVALID));
        assert_eq!(tags.insert(u32::MAX), Err(ERR_INVALID));
        assert_eq!(tags, EventTags::default());
        assert!(!tags.contains(EventTags::LIMIT) && !tags.contains(u32::MAX));
    }

    #[test]
    fn the_network_event_tags_fit() {
        let mut tags = EventTags::default();
        for tag in [wire::EVENT_HTTP_DONE_TAG, wire::EVENT_SERVER_TAG] {
            tags.insert(tag).unwrap();
            assert!(tags.contains(tag));
        }
    }
}
