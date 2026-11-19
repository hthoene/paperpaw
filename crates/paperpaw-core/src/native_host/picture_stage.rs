//! The host's side of the stage `core.picture` (`docs/abi/document.md` 5):
//! the picture pump asks its subscribers (at most two, in stage order) once
//! for each picture that got an image id (a plugin's own, before that plugin
//! is told of it; a picture of a book's page, a comic's page, a cover, the
//! sleep screen's picture, in the pass after its id was made), with the
//! picture's place, size and a histogram of its grays. The first answer
//! that says something is kept with the image (`image_service::tone`): a tone
//! curve of 256 grays, dots or grays. A subscriber that fails, answers badly
//! or says nothing changes nothing: the picture is drawn as it is.
//!
//! The stage is asked between callbacks only, so the id of a document's
//! picture is in its plugin's hands before the answer. A plugin that draws
//! it in a later frame (the reader's pictures come in the frame after the
//! page's text) draws it with the answer; one that drew it in the frame that
//! made the id is drawn once more ([`ask`] says so).
use super::{
    declaration_store::DeclarationStore,
    image_service::tone::{self, Shown, Wanted},
    stage_broker::{Schema, Subscription},
    stages_wire::core_picture_v1 as stage,
    ui_input::{BrokerRunner, FilterRunner},
};
use alloc::string::String;
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;
use paperpaw_sdk::builtin::slots;

/// The stage as the broker runs it: its bounds are the stage's own.
fn schema() -> Schema {
    Schema {
        key: stage::KEY,
        major: u32::from(stage::MAJOR),
        max_subscribers: stage::MAX_SUBSCRIBERS,
        max_input: stage::MAX_INPUT,
        max_reply: stage::MAX_REPLY,
        reply_rows: 1,
        input: |bytes| stage::input_decode(bytes).map(|_| ()),
        reply: |bytes| stage::reply_decode(bytes).map(|_| ()),
    }
}

/// The subscribers of the stage in stage order, the first two.
fn subscriptions(store: &DeclarationStore) -> Result<Vec<Subscription<'static>>, i32> {
    let mut found = store.stage_subscribers(stage::KEY)?;
    paperpaw_sort::sort_unstable_by_key(&mut found, |s| (s.order, s.plugin, s.index));
    found.truncate(stage::MAX_SUBSCRIBERS);
    let mut rows = Vec::new();
    rows.try_reserve_exact(found.len())
        .map_err(|_| ERR_NO_MEMORY)?;
    rows.extend(found.iter().map(|subscriber| Subscription {
        stage: stage::KEY,
        plugin: subscriber.owner,
        token: subscriber.token,
        order: subscriber.order,
        index: subscriber.index,
        signature: subscriber.signature,
        enabled: true,
        style_granted: false,
    }));
    Ok(rows)
}

/// Asks `subscribers` in turn about `wanted`, shown at `place` and of the
/// book `book` (its path on root 1, empty for none), and keeps the first
/// answer that says something with its image in `table`. `true` where a
/// picture that has been drawn already got an answer: what shows it is to be
/// drawn again.
#[inline(never)] // own frame: the stage's buffers stay out of the pump's
pub(crate) fn ask(
    runner: &mut dyn FilterRunner,
    subscribers: &[Subscription<'_>],
    storage: &dyn Storage,
    table: &super::image_service::ImageTable,
    wanted: &Wanted,
    (place, book): (u32, &str),
) -> bool {
    // A panel of a page already asked for: that answer, nothing read.
    if let Some(says) = table.tone_shared(wanted) {
        return says && wanted.drawn;
    }
    // A picture that cannot be read now is drawn as it is.
    let Some((histogram, flags)) = tone::histogram(storage, &wanted.path) else {
        return false;
    };
    let ask = stage::Record::PictureAsk(stage::PictureAsk::new(
        place,
        wanted.width,
        wanted.height,
        flags,
        &histogram,
        book,
    ));
    let mut payload = [0_u8; stage::MAX_INPUT];
    let Ok(length) = stage::input_encode(&[ask], &mut payload) else {
        return false;
    };
    let mut reply = [0_u8; stage::MAX_REPLY];
    for subscriber in subscribers {
        let Ok(written) = runner.call(subscriber, &payload[..length], &mut reply) else {
            continue;
        };
        let answer = reply
            .get(..written)
            .and_then(|bytes| stage::reply_decode(bytes).ok())
            .and_then(|mut rows| rows.next());
        let Some(Ok(stage::Record::PictureTone(answer))) = answer else {
            continue;
        };
        // Empty: this subscriber leaves the picture alone, the next is asked.
        if answer.tone.is_empty() && answer.flags == 0 {
            continue;
        }
        if table.tone_answer(wanted.owner, wanted.id, answer.tone, answer.flags) {
            return wanted.drawn;
        }
    }
    false
}

impl crate::Kernel {
    /// Asks the picture stage for every picture not asked yet. Called by
    /// the picture pump, outside every callback, before a plugin is told
    /// that its picture is ready. Without a subscriber nothing is read and
    /// no plugin runs, and the declarations are not searched again until
    /// they change. `true` where a picture already drawn got an answer.
    #[inline(never)]
    pub(crate) fn picture_tones(&self) -> bool {
        let shared = &self.shared;
        let Some(hosts) = shared.native_instances.hosts() else {
            return false;
        };
        let table = hosts.images();
        if !table.tone_pending() {
            return false;
        }
        let store = shared
            .registry
            .native_declarations
            .as_deref()
            .and_then(|directory| directory.store.try_borrow().ok());
        let revision = store.as_ref().map(|store| store.revision());
        if revision.is_some_and(|revision| table.tones_unsubscribed_at(revision)) {
            table.tones_skip();
            return false;
        }
        let subscribers = store
            .and_then(|store| subscriptions(&store).ok())
            .unwrap_or_default();
        let (Some(depth), false) = (shared.native_depth.as_ref(), subscribers.is_empty()) else {
            if let Some(revision) = revision {
                table.tones_unsubscribed(revision);
            }
            table.tones_skip();
            return false;
        };
        let env = shared.place(shared.now);
        let schema = schema();
        let mut runner = BrokerRunner {
            schema: &schema,
            depth: depth.as_depth(),
            registry: &shared.native_instances,
            env: &env,
        };
        let mut again = false;
        while let Some(wanted) = table.tone_wanted() {
            let (place, book) = self.picture_shown(&wanted);
            again |= ask(
                &mut runner,
                &subscribers,
                shared.storage.as_ref(),
                table,
                &wanted,
                (place, &book),
            );
        }
        again
    }

    /// Where `wanted` is shown (`picture.ask.place`) and its book's path on
    /// root 1 (empty for none, and where there is no room for it).
    fn picture_shown(&self, wanted: &Wanted) -> (u32, String) {
        let instances = &self.shared.native_instances;
        let mut book = String::new();
        let mut whole_pages = false;
        if let Some((document, _)) = wanted.document
            && let Some(documents) = instances.hosts().and_then(|hosts| hosts.documents())
        {
            documents
                .0
                .picture_book(wanted.owner, document, &mut |whole, path| {
                    whole_pages = whole;
                    if book.try_reserve_exact(path.len()).is_ok() {
                        book.push_str(path);
                    }
                });
        }
        let sleeping = self.stack.last().is_some_and(|top| {
            top.slot.as_ref() == Some(&slots::SLEEP)
                && instances
                    .id(wanted.owner)
                    .is_some_and(|id| top.id.namespace() == id.as_str())
        });
        let bounds = self.framebuffer.bounds();
        let shown = Shown {
            whole_pages,
            sleeping,
            long_side: u32::from(bounds.width.max(bounds.height)),
        };
        (wanted.place(shown), book)
    }
}

#[cfg(test)]
#[path = "picture_stage_tests.rs"]
mod tests;
