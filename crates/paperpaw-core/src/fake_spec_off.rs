//! No test's screen outside the feature `testing`.

use crate::Id;

#[derive(Debug)]
pub(crate) enum FakeSpec {}

impl FakeSpec {
    pub(crate) fn id(&self) -> &Id {
        match *self {}
    }
}
