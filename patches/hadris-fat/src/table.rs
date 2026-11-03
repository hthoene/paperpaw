#[cfg(not(feature = "small"))]
use alloc::collections::BTreeMap;

use hadris_fs::{ErrorKind, NodeId};

/// Modified for PaperPaw (feature `small`): the table is a vector kept
/// sorted by id, not a `BTreeMap`. A driver pins a few nodes at a time,
/// and the B-tree's code took about 5 KB of flash on a microcontroller.
#[cfg(feature = "small")]
type BTreeMap<K, V> = sorted::SortedVec<K, V>;

#[cfg(feature = "small")]
mod sorted {
    use alloc::vec::Vec;
    use core::ops::RangeInclusive;

    /// The part of `BTreeMap`'s interface the table uses, on a vector
    /// sorted by key: a binary search finds a key.
    pub(crate) struct SortedVec<K, V>(Vec<(K, V)>);

    impl<K: Ord + Copy, V> SortedVec<K, V> {
        pub(crate) const fn new() -> Self {
            Self(Vec::new())
        }

        pub(crate) fn len(&self) -> usize {
            self.0.len()
        }

        pub(crate) fn is_empty(&self) -> bool {
            self.0.is_empty()
        }

        fn at(&self, key: &K) -> Result<usize, usize> {
            self.0.binary_search_by(|(k, _)| k.cmp(key))
        }

        pub(crate) fn get(&self, key: &K) -> Option<&V> {
            self.at(key).ok().map(|i| &self.0[i].1)
        }

        pub(crate) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
            self.at(key).ok().map(|i| &mut self.0[i].1)
        }

        pub(crate) fn insert(&mut self, key: K, value: V) -> Option<V> {
            match self.at(&key) {
                Ok(i) => Some(core::mem::replace(&mut self.0[i].1, value)),
                Err(i) => {
                    self.0.insert(i, (key, value));
                    None
                }
            }
        }

        pub(crate) fn remove(&mut self, key: &K) -> Option<V> {
            self.at(key).ok().map(|i| self.0.remove(i).1)
        }

        pub(crate) fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
            self.0.iter().map(|(k, v)| (k, v))
        }

        pub(crate) fn range(&self, range: RangeInclusive<K>) -> impl Iterator<Item = (&K, &V)> {
            let from = self.at(range.start()).unwrap_or_else(|i| i);
            let to = *range.end();
            self.0[from..]
                .iter()
                .take_while(move |(k, _)| *k <= to)
                .map(|(k, v)| (k, v))
        }
    }

    impl<'a, K, V> IntoIterator for &'a mut SortedVec<K, V> {
        type Item = (&'a K, &'a mut V);
        type IntoIter = core::iter::Map<
            core::slice::IterMut<'a, (K, V)>,
            fn(&'a mut (K, V)) -> (&'a K, &'a mut V),
        >;

        fn into_iter(self) -> Self::IntoIter {
            self.0.iter_mut().map(|(k, v)| (&*k, v))
        }
    }
}

/// Per-node driver state with a pin count per node, for pinned and open
/// nodes. An entry goes when its last pin does, or on `remove`.
pub(crate) struct Table<V> {
    nodes: BTreeMap<NodeId, (u32, V)>,
    limit: Option<usize>,
}

impl<V> Table<V> {
    /// An empty table holding at most `limit` entries.
    pub(crate) const fn new(limit: Option<usize>) -> Self {
        Self {
            nodes: BTreeMap::new(),
            limit,
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.nodes.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub(crate) fn get(&self, id: NodeId) -> Option<&V> {
        self.nodes.get(&id).map(|(_, value)| value)
    }

    pub(crate) fn get_mut(&mut self, id: NodeId) -> Option<&mut V> {
        self.nodes.get_mut(&id).map(|(_, value)| value)
    }

    /// The pin count of `id`, 0 when absent.
    pub(crate) fn pins(&self, id: NodeId) -> u32 {
        self.nodes.get(&id).map_or(0, |(pins, _)| *pins)
    }

    /// The first entry for which `pred` holds.
    pub(crate) fn find(&self, mut pred: impl FnMut(NodeId, &V) -> bool) -> Option<NodeId> {
        self.nodes
            .iter()
            .find(|(id, (_, value))| pred(**id, value))
            .map(|(id, _)| *id)
    }

    /// The first entry with an id in `from..=to` for which `pred` holds.
    pub(crate) fn find_in(
        &self,
        from: NodeId,
        to: NodeId,
        mut pred: impl FnMut(NodeId, &V) -> bool,
    ) -> Option<NodeId> {
        if from > to {
            return None;
        }
        self.nodes
            .range(from..=to)
            .find(|(id, (_, value))| pred(**id, value))
            .map(|(id, _)| *id)
    }

    pub(crate) fn for_each_mut(&mut self, mut f: impl FnMut(NodeId, &mut V)) {
        for (id, (_, value)) in &mut self.nodes {
            f(*id, value);
        }
    }

    /// Adds `id` with one pin, or replaces its state and keeps its pins.
    /// [`ErrorKind::LimitExceeded`] when a new entry would pass the limit.
    pub(crate) fn insert(&mut self, id: NodeId, value: V) -> Result<(), ErrorKind> {
        if let Some((_, old)) = self.nodes.get_mut(&id) {
            *old = value;
            return Ok(());
        }
        if self.limit.is_some_and(|limit| self.nodes.len() >= limit) {
            return Err(ErrorKind::LimitExceeded);
        }
        self.nodes.insert(id, (1, value));
        Ok(())
    }

    /// Adds a pin and returns the new count, or `None` when absent.
    pub(crate) fn pin(&mut self, id: NodeId) -> Option<u32> {
        let (pins, _) = self.nodes.get_mut(&id)?;
        *pins = pins.saturating_add(1);
        Some(*pins)
    }

    /// Drops a pin and returns the remaining count, or `None` when absent.
    /// The entry goes at zero.
    pub(crate) fn unpin(&mut self, id: NodeId) -> Option<u32> {
        let (pins, _) = self.nodes.get_mut(&id)?;
        *pins = pins.saturating_sub(1);
        let pins = *pins;
        if pins == 0 {
            self.nodes.remove(&id);
        }
        Some(pins)
    }

    pub(crate) fn remove(&mut self, id: NodeId) -> Option<V> {
        self.nodes.remove(&id).map(|(_, value)| value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(raw: u64) -> NodeId {
        NodeId::new(raw).unwrap()
    }

    #[test]
    fn pins_and_removal() {
        let mut table = Table::new(None);
        assert!(table.is_empty());
        assert_eq!(table.insert(id(1), 10), Ok(()));
        assert_eq!(table.pins(id(1)), 1);
        assert_eq!(table.pin(id(1)), Some(2));
        assert_eq!(table.pin(id(9)), None);
        assert_eq!(table.unpin(id(1)), Some(1));
        assert_eq!(table.get(id(1)), Some(&10));
        assert_eq!(table.unpin(id(1)), Some(0));
        assert_eq!(table.get(id(1)), None);
        assert_eq!(table.unpin(id(1)), None);
        table.insert(id(2), 20).unwrap();
        table.insert(id(3), 30).unwrap();
        table.pin(id(3));
        table.insert(id(3), 31).unwrap();
        assert_eq!(table.pins(id(3)), 2);
        *table.get_mut(id(2)).unwrap() += 1;
        assert_eq!(table.find(|_, value| *value == 21), Some(id(2)));
        let mut sum = 0;
        table.for_each_mut(|_, value| {
            *value += 1;
            sum += *value;
        });
        assert_eq!(sum, 22 + 32);
        assert_eq!(table.remove(id(3)), Some(32));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn the_limit_caps_new_entries() {
        let mut table = Table::new(Some(1));
        table.insert(id(1), 1).unwrap();
        assert_eq!(table.insert(id(2), 2), Err(ErrorKind::LimitExceeded));
        assert_eq!(table.insert(id(1), 3), Ok(()));
        table.remove(id(1));
        assert_eq!(table.insert(id(2), 2), Ok(()));
    }
}
