//! The event tags an instance subscribed to, kept as a small bit set so that
//! "is this plugin subscribed to event E" is a read without allocation
//! (`core.net` asks it before it accepts a request or a server).
use paperpaw_native::host_glue::ERR_INVALID;

/// A set of event wire tags below [`EventTags::LIMIT`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct EventTags([u64; 2]);

impl EventTags {
    /// Tags from this value on cannot be held: the wire's event tags end
    /// below it (the last is 112). Two words, not four: the set is in every
    /// owner's row (#248, B). An event tag at or past it fails the build.
    pub(crate) const LIMIT: u32 = 128;

    /// The tags in the set for which `has` says yes (asked once for each tag
    /// the set can hold, word by word: no tag range is tested).
    pub(crate) fn collect(mut has: impl FnMut(u32) -> bool) -> Self {
        let mut set = Self::default();
        for (first, word) in (0u32..).step_by(64).zip(set.0.iter_mut()) {
            for bit in 0..64u32 {
                if has(first + bit) {
                    *word |= 1 << bit;
                }
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

    /// Whether no tag is in the set.
    pub(crate) fn is_empty(&self) -> bool {
        self.0.iter().all(|word| *word == 0)
    }

    /// Whether `tag` was added. A tag beyond the set was never added.
    pub(crate) fn contains(&self, tag: u32) -> bool {
        self.0
            .get((tag / 64) as usize)
            .is_some_and(|word| word & (1 << (tag % 64)) != 0)
    }
}

// Every event tag fits the set: a later event at or past the limit fails the
// build here, not a subscription on the device. Checked through every 16-bit
// tag (the whole 32-bit range would cost the build minutes; the wire's tags
// are far below 65,536).
const _: () = {
    let mut tag = EventTags::LIMIT;
    while tag <= u16::MAX as u32 {
        assert!(
            !super::events::is_event_tag(tag),
            "an event tag past EventTags::LIMIT"
        );
        tag += 1;
    }
};

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
        tags.insert(127).unwrap();
        assert!(tags.contains(99) && tags.contains(0) && tags.contains(127));
        assert!(!tags.contains(100) && !tags.contains(1) && !tags.contains(126));
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
    fn collect_asks_every_tag_the_set_can_hold_once() {
        let mut asked = [0u8; EventTags::LIMIT as usize];
        let tags = EventTags::collect(|tag| {
            asked[tag as usize] += 1;
            matches!(tag, 0 | 63 | 64 | 99 | 127)
        });
        assert!(asked.iter().all(|&n| n == 1));
        for tag in [0, 63, 64, 99, 127] {
            assert!(tags.contains(tag), "{tag}");
        }
        assert!(!tags.contains(1) && !tags.contains(65) && !tags.contains(100));
    }

    #[test]
    fn the_set_is_two_words() {
        // #248 B: it is in every owner's row; 256 bits held 144 never used.
        assert_eq!(core::mem::size_of::<EventTags>(), 16);
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
