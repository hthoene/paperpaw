//! The cat's picture on interface 7: which file to ask the host to prepare,
//! what its answers lead to, and the picture that is ready.
//!
//! The order is interface 6's: one's own picture of the mood from the folder
//! the setting `reading-goal.pictures` names (`<mood>.png`, `.jpg`, `.jpeg`
//! in either case), else PaperPaw's cats, installed with the plugin
//! (`pictures/<mood>.png`, read from the plugin's own install folder). With
//! neither, the mood is put in words. One request is open at a time and one
//! picture is held: the one of the mood the widget shows.
//!
//! The decisions here need no host: [`Pictures`] says what to ask next and
//! what an answer means; `native_plugin` sends the commands.
use alloc::string::String;
use paperpaw_sdk::native::image::{ImageId, ImageStatus};

use crate::model::Mood;

/// The file extensions of one's own pictures, in the order they are tried.
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "PNG", "JPG", "JPEG"];
/// The longest path of a request (`command.image_prepare`).
pub(crate) const PATH_MAX: usize = 512;
/// Where the cats are in the plugin's install folder.
const CATS: &str = "pictures";

/// Where a candidate file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Source {
    /// The card (the library root): one's own folder.
    Card,
    /// The plugin's install folder: the cats.
    Bundle,
}

/// One file to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub source: Source,
    pub path: String,
}

/// How many files there are to try with a folder set and without: the
/// extensions of one's own picture, then the cat.
pub(crate) const fn steps(own: bool) -> usize {
    if own { EXTENSIONS.len() + 1 } else { 1 }
}

/// The `step`-th file to try for `mood`, where `folder` is the one's own
/// folder (without slashes at its ends, empty: none). `None` for a step past
/// the last ([`steps`]) and where a path would be too long or the heap
/// refuses: that file is left out.
pub(crate) fn candidate(folder: &str, mood: Mood, step: usize) -> Option<Candidate> {
    let own = !folder.is_empty();
    if step >= steps(own) {
        return None;
    }
    let (source, path) = match (own, EXTENSIONS.get(step)) {
        (true, Some(extension)) => (
            Source::Card,
            join(&[folder, "/", mood.name(), ".", extension]),
        ),
        _ => (Source::Bundle, join(&[CATS, "/", mood.name(), ".png"])),
    };
    path.map(|path| Candidate { source, path })
}

/// `parts` as one path, or `None` where it is longer than a request takes
/// or the heap refuses.
fn join(parts: &[&str]) -> Option<String> {
    let total = parts.iter().map(|part| part.len()).sum::<usize>();
    if total > PATH_MAX {
        return None;
    }
    let mut out = String::new();
    out.try_reserve_exact(total).ok()?;
    for part in parts {
        out.push_str(part);
    }
    Some(out)
}

/// The number a request is made with: the mood and the step, so that an
/// answer to a request that is not the open one is known.
pub(crate) const fn cookie(mood: Mood, step: usize) -> u32 {
    ((mood as u32) << 8) | (step as u32 & 0xff)
}

/// What the host's answer to the request in flight leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Next {
    /// Not the open request's answer (or none open).
    Ignore,
    /// The picture is ready; the one held before (if any) is to be released.
    Ready {
        image: ImageId,
        release: Option<ImageId>,
    },
    /// Not this file: ask the next step.
    Again { mood: Mood, step: usize },
    /// A refusal for want of memory: nothing is held for the mood now, and
    /// it is asked again at the next minute.
    Retry,
    /// Nothing more to try: the mood stays in words.
    Words,
}

/// The picture of one mood and what is asked for it.
#[derive(Debug, Default)]
pub(crate) struct Pictures {
    /// The mood the held picture is of.
    held: Option<(Mood, ImageId, (u32, u32))>,
    /// The request open: the mood and the step.
    open: Option<(Mood, usize)>,
    /// The mood nothing was found for (until the folder setting changes).
    none: Option<Mood>,
}

impl Pictures {
    /// The picture to draw for `mood`, if the held one is of it.
    pub(crate) fn of(&self, mood: Mood) -> Option<ImageId> {
        self.held
            .filter(|(held, ..)| *held == mood)
            .map(|(_, image, _)| image)
    }

    /// The size in pixels of the picture held for `mood`, as the host made it.
    pub(crate) fn size_of(&self, mood: Mood) -> Option<(u32, u32)> {
        self.held
            .filter(|(held, ..)| *held == mood)
            .map(|(_, _, size)| size)
    }

    /// Whether to start asking for the picture of `mood`: not when it is held,
    /// asked for, or known to be missing.
    pub(crate) fn wants(&self, mood: Mood) -> bool {
        self.of(mood).is_none() && self.open.is_none() && self.none != Some(mood)
    }

    /// The request for step `step` of `mood` is made.
    pub(crate) fn asked(&mut self, mood: Mood, step: usize) {
        self.open = Some((mood, step));
    }

    /// The folder setting changed: what was found (or not) may differ now.
    /// The picture held, if any, is to be released by the caller.
    pub(crate) fn forget(&mut self) -> Option<ImageId> {
        self.none = None;
        // An answer to a request made before is no longer the one waited for.
        self.open = None;
        self.held.take().map(|(_, image, _)| image)
    }

    /// No file is left to ask for: the mood stays in words.
    pub(crate) fn gave_up(&mut self, mood: Mood) {
        self.none = Some(mood);
    }

    /// The host's answer.
    pub(crate) fn ready(
        &mut self,
        (cookie_of, status, image, size): (u32, ImageStatus, Option<ImageId>, (u32, u32)),
        own: bool,
    ) -> Next {
        let Some((mood, step)) = self.open else {
            return Next::Ignore;
        };
        if cookie_of != cookie(mood, step) {
            return Next::Ignore;
        }
        self.open = None;
        match (status, image) {
            (ImageStatus::Ready, Some(image)) => {
                let release = self.held.map(|(_, held, _)| held);
                self.held = Some((mood, image, size));
                Next::Ready { image, release }
            }
            (ImageStatus::NoMemory | ImageStatus::CardFailure, _) => Next::Retry,
            _ => {
                let next = step.saturating_add(1);
                if next < steps(own) {
                    Next::Again { mood, step: next }
                } else {
                    self.none = Some(mood);
                    Next::Words
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
