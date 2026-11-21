//! The catalog's job, one step at a time: what it does to the library's
//! files.

use crate::Place;
use alloc::string::String;

use crate::kit::library;
use paperpaw_sdk::builtin::{settings, slots};

/// What the kernel does to the catalog: walk the library, or catch up;
/// before either, once, move the books' details and covers that earlier
/// versions kept into the folders split by keys ([`library::split`]).
pub(super) enum CatalogJob {
    Walk(library::catalog::Walk),
    Rewrite(library::catalog::Rewrite),
    /// The books opened last that the catalog lacks, appended to it before
    /// the first walk after a start ([`library::catalog::CatchUp`]).
    CatchUp(library::catalog::CatchUp),
    Split(library::split::Split),
    /// The books sorted for the view of them ([`library::sorted`]), after
    /// the catalog was made anew.
    Sort(library::sorted::Build),
    /// A first walk that put the books it found so far into the catalog
    /// (`Walk::published`), set aside while they are sorted for the views
    /// (`None` once they are): then views hear of them and it goes on.
    Shown(
        alloc::boxed::Box<library::catalog::Walk>,
        Option<library::sorted::Build>,
    ),
}

impl CatalogJob {
    pub(super) fn step(&mut self, env: &Place<'_>) -> Result<bool, paperpaw_sdk::StorageError> {
        match self {
            CatalogJob::Walk(walk) => walk.step(env),
            CatalogJob::Rewrite(rewrite) => rewrite.step(env),
            CatalogJob::CatchUp(catch_up) => catch_up.step(env),
            CatalogJob::Split(split) => split.step(env.storage(), env.book_data()),
            CatalogJob::Sort(sort) => sort.step(env),
            CatalogJob::Shown(_, sort) => {
                match sort.as_mut().map(|s| s.step(env)) {
                    Some(Ok(false)) => {}
                    // Not sorted: the views still hear of the books, and
                    // ask for them sorted anew.
                    Some(Err(_)) => {
                        if let Some(failed) = sort.take() {
                            failed.abort(env.storage());
                        }
                    }
                    Some(Ok(true)) | None => *sort = None,
                }
                Ok(false)
            }
        }
    }

    /// How far a walk is, in parts per million ([`library::catalog::Walk::progress_ppm`]),
    /// `None` for any other job.
    pub(super) fn progress_ppm(&self) -> Option<u32> {
        match self {
            CatalogJob::Walk(walk) => Some(walk.progress_ppm()),
            CatalogJob::Shown(walk, _) => Some(walk.progress_ppm()),
            _ => None,
        }
    }

    /// A walk (set aside or not).
    pub(super) fn is_walk(&self) -> bool {
        matches!(self, CatalogJob::Walk(_) | CatalogJob::Shown(..))
    }

    pub(super) fn abort(self, storage: &dyn paperpaw_sdk::Storage) {
        match self {
            CatalogJob::Walk(walk) => walk.abort(storage),
            CatalogJob::Rewrite(rewrite) => rewrite.abort(storage),
            // Each move is whole: the next goes on from what is left.
            CatalogJob::Split(_) | CatalogJob::CatchUp(_) => {}
            CatalogJob::Sort(sort) => sort.abort(storage),
            CatalogJob::Shown(walk, sort) => {
                if let Some(sort) = sort {
                    sort.abort(storage);
                }
                (*walk).abort(storage);
            }
        }
    }

    pub(super) fn pause(&mut self, storage: &dyn paperpaw_sdk::Storage) {
        match self {
            CatalogJob::Walk(walk) => walk.pause(),
            CatalogJob::Rewrite(rewrite) => rewrite.pause(),
            CatalogJob::Split(_) | CatalogJob::CatchUp(_) => {}
            CatalogJob::Sort(sort) | CatalogJob::Shown(_, Some(sort)) => sort.pause(storage),
            CatalogJob::Shown(_, None) => {}
        }
    }
}

impl super::Catalog {
    /// Stops the job under way, for one begun anew: what it took is given
    /// back, a move of the details it cut short is wanted again.
    // Its own frame, as `start`: each moves a job, and the step's frame
    // stays under the firmware's frame limit for host code.
    #[inline(never)]
    pub(super) fn stop(&self, job: &mut Option<CatalogJob>, storage: &dyn paperpaw_sdk::Storage) {
        let Some(stopped) = job.take() else {
            return;
        };
        let walk = stopped.is_walk();
        self.split
            .set(self.split.get() || matches!(stopped, CatalogJob::Split(_)));
        stopped.abort(storage);
        self.give_back(walk);
    }

    /// Starts the job that is wanted, where none runs: a walk, catching up,
    /// or sorting the books. `Some` with how the step ends where there is
    /// nothing to do or the job could not start.
    #[inline(never)]
    pub(super) fn start(
        &self,
        env: &Place<'_>,
        job: &mut Option<CatalogJob>,
        out: &mut crate::Asks,
    ) -> Option<paperpaw_sdk::work::Step> {
        use paperpaw_sdk::work::Step;
        let fresh = core::mem::take(&mut *self.fresh.borrow_mut());
        // The first walk after a start: the books opened last that a
        // catalog there lacks are appended to it first, so that they show
        // before the walk ends. The walk stays wanted.
        let catch_up = self.walk.get()
            && !self.caught.replace(true)
            && library::catalog::state(env.storage()) != library::catalog::State::Missing;
        let walk = !catch_up && self.walk.replace(false);
        let started = if catch_up {
            library::catalog::CatchUp::start(env).map(CatalogJob::CatchUp)
        } else if walk {
            library::catalog::Walk::start(env, fresh.clone()).map(CatalogJob::Walk)
        } else if !fresh.is_empty() || library::catalog::log_is_long(env.storage()) {
            library::catalog::Rewrite::start(env, fresh.clone()).map(CatalogJob::Rewrite)
        } else if let Some(archive) = self.sort_wanted(env) {
            library::sorted::Build::start(env, &archive).map(CatalogJob::Sort)
        } else {
            self.urgent.set(false);
            return Some(Step::Done);
        };
        *self.taken.borrow_mut() = fresh;
        match started {
            Ok(started) => {
                *job = Some(started);
                None
            }
            Err(error) => {
                self.give_back(walk);
                out.push(crate::Ask::emit(
                    &paperpaw_sdk::builtin::events::CATALOG_FAILED,
                    (),
                ));
                Some(Step::Failed(alloc::format!("catalog: {error:?}")))
            }
        }
    }
}

/// After a step that did not end the job: a first walk that put the books
/// it found so far into the catalog is set aside (paused) while they are
/// sorted for the views (where one asked), which then hear of them
/// (`CATALOG_CHANGED`, as of a catalog made anew); the walk goes on.
pub(super) fn after_step(
    catalog: &super::Catalog,
    env: &Place<'_>,
    job: &mut Option<CatalogJob>,
    out: &mut crate::Asks,
) -> paperpaw_sdk::work::Step {
    use paperpaw_sdk::builtin::events;
    let published = match job {
        Some(CatalogJob::Walk(walk)) => walk.published(),
        _ => false,
    };
    match job.take() {
        Some(CatalogJob::Walk(mut walk)) if published => {
            let sort = catalog
                .sort_wanted(env)
                .and_then(|archive| library::sorted::Build::start(env, &archive).ok());
            *job = Some(match sort {
                Some(sort) => {
                    walk.pause();
                    CatalogJob::Shown(alloc::boxed::Box::new(walk), Some(sort))
                }
                None => {
                    out.push(crate::Ask::emit(&events::CATALOG_CHANGED, ()));
                    CatalogJob::Walk(walk)
                }
            });
        }
        Some(CatalogJob::Shown(walk, None)) => {
            out.push(crate::Ask::emit(&events::CATALOG_CHANGED, ()));
            *job = Some(CatalogJob::Walk(*walk));
        }
        other => *job = other,
    }
    paperpaw_sdk::work::Step::Continue
}

/// Whether the catalog's job waits (paused, it goes on where it was):
/// while a book is read (the reader is the screen shown, overlays over it
/// aside), its memory and time are the reader's. Beside the library it
/// goes on, also on a device of little memory (the X4): the library holds
/// a few kilobytes of the sorted books ([`library::sorted`]), and says
/// they are being prepared until the job made them.
pub(super) fn waits(env: &Place<'_>) -> bool {
    let shown = env.stack().iter().rev().find(|entry| !entry.overlay);
    shown.is_some_and(|entry| entry.slot.as_ref() == Some(&slots::READER))
}

/// The library's folder is there from the first start on: made where it
/// is not (never removed), so that books copied onto the card or sent
/// over Wi-Fi have their place from the start.
pub(super) fn make_library_folder(env: &Place<'_>) {
    let folder = env.settings().read(&settings::LIBRARY_FOLDER);
    let folder = folder.trim_matches('/');
    if folder.is_empty() || paperpaw_sdk::files::is_dir(env.storage(), folder) {
        return;
    }
    let mut made = String::new();
    for part in folder.split('/') {
        made = paperpaw_sdk::files::join(&made, part);
        if !paperpaw_sdk::files::is_dir(env.storage(), &made) {
            let _ = env.storage().create_dir(&made);
        }
    }
}

/// The sorted books, for a view leaving an archive out
/// ([`library::sorted`]): made where they are not of the library there
/// is now. And the walk wanted again after a wake.
pub(super) fn register_sorted(r: &mut crate::Staging) -> Result<(), paperpaw_sdk::RegisterError> {
    use paperpaw_sdk::builtin::events;
    // Woken from a light sleep (no start): walked again as after a start,
    // for books copied while it slept (the card in a computer). By the
    // first view of the library to show; at once where one is open (it
    // does not ask again; under a book the walk waits until it closes).
    // A walk under way at the sleep starts again (it may have passed the
    // folder a book came into); nothing else under way is started again.
    // Not where the shell said the storage came through the sleep as it
    // was left (`Kernel::resume_storage_kept`: a card that kept its power,
    // #305): nothing was copied, nothing is walked again, and a walk under
    // way goes on where it was.
    r.on(events::RESUME, |k, p| {
        // The time asleep is no silence of the catalog work (#460).
        let now = k.shared.now.millis();
        k.core.catalog.seen.woke(now);
        if k.storage_kept {
            return;
        }
        k.with_core_event(&events::RESUME, p, |env, core, _, out| {
            core.catalog.walked.set(false);
            let walking = core
                .catalog
                .job
                .borrow()
                .as_ref()
                .is_some_and(CatalogJob::is_walk);
            let open = env
                .stack()
                .iter()
                .any(|entry| entry.slot.as_ref() == Some(&paperpaw_sdk::builtin::slots::LIBRARY));
            if walking {
                core.catalog.want_walk(true, true, out);
            } else if open && !env.formats().is_empty() {
                // Other work under way (sorting, catching up) ends first.
                core.catalog.walk.set(true);
                core.catalog.declare(true, out);
            }
        });
    })?;
    r.on(events::SORTED_WANTED, |k, p| {
        k.with_core_event(&events::SORTED_WANTED, p, |env, core, wanted, out| {
            let mut archives = core.catalog.archives.borrow_mut();
            library::sorted::asked(env.storage(), &mut archives, &wanted.archive);
            drop(archives);
            if core.catalog.sort_wanted(env).is_some() {
                core.catalog.declare(true, out);
            }
        });
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::boxed::Box;

    #[test]
    fn only_a_walk_has_a_progress() {
        let walk = library::catalog::Walk::default();
        assert_eq!(CatalogJob::Walk(walk).progress_ppm(), Some(0));
        let shown = CatalogJob::Shown(Box::default(), None);
        assert_eq!(shown.progress_ppm(), Some(0));
        assert_eq!(
            CatalogJob::Split(library::split::Split::new()).progress_ppm(),
            None
        );
    }
}
