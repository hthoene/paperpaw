//! The host's side of the stage `core.picture` (`docs/abi/document.md` 5):
//! the picture pump asks its subscribers (at most two, in stage order) once
//! for each picture a plugin prepared, before that plugin is told of it, with
//! the picture's place, size and a histogram of its grays. The first answer
//! that says something is kept with the image (`image_service::tone`): a tone
//! curve of 256 grays, dots or grays. A subscriber that fails, answers badly
//! or says nothing changes nothing: the picture is drawn as it is.
use super::{
    declaration_store::DeclarationStore,
    image_service::tone::{self, Wanted},
    stage_broker::{Schema, Subscription},
    stages_wire::core_picture_v1 as stage,
    ui_input::{BrokerRunner, FilterRunner},
};
use alloc::vec::Vec;
use paperpaw_native::host_glue::ERR_NO_MEMORY;
use paperpaw_platform::Storage;

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

/// Asks `subscribers` in turn about `wanted` and keeps the first answer that
/// says something with its image in `table`.
#[inline(never)] // own frame: the stage's buffers stay out of the pump's
pub(crate) fn ask(
    runner: &mut dyn FilterRunner,
    subscribers: &[Subscription<'_>],
    storage: &dyn Storage,
    table: &super::image_service::ImageTable,
    wanted: &Wanted,
) {
    // A picture that cannot be read now is drawn as it is.
    let Some((histogram, flags)) = tone::histogram(storage, &wanted.path) else {
        return;
    };
    let ask = stage::Record::PictureAsk(stage::PictureAsk::new(
        tone::PLACE_UI,
        wanted.width,
        wanted.height,
        flags,
        &histogram,
        "",
    ));
    let mut payload = [0_u8; 96];
    let Ok(length) = stage::input_encode(&[ask], &mut payload) else {
        return;
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
            return;
        }
    }
}

impl crate::Kernel {
    /// Asks the picture stage for every prepared picture not asked yet.
    /// Called by the picture pump, outside every callback, before a plugin
    /// is told that its picture is ready. Without a subscriber nothing is
    /// read and no plugin runs.
    #[inline(never)]
    pub(crate) fn picture_tones(&self) {
        let shared = &self.shared;
        let Some(hosts) = shared.native_instances.hosts() else {
            return;
        };
        let table = hosts.images();
        if !table.tone_pending() {
            return;
        }
        let subscribers = shared
            .registry
            .native_declarations
            .as_deref()
            .and_then(|directory| directory.store.try_borrow().ok())
            .and_then(|store| subscriptions(&store).ok())
            .unwrap_or_default();
        let (Some(depth), false) = (shared.native_depth.as_ref(), subscribers.is_empty()) else {
            table.tones_skip();
            return;
        };
        let env = shared.place(shared.now);
        let schema = schema();
        let mut runner = BrokerRunner {
            schema: &schema,
            depth: depth.as_depth(),
            registry: &shared.native_instances,
            env: &env,
        };
        while let Some(wanted) = table.tone_wanted() {
            ask(
                &mut runner,
                &subscribers,
                shared.storage.as_ref(),
                table,
                &wanted,
            );
        }
    }
}

#[cfg(test)]
#[path = "picture_stage_tests.rs"]
mod tests;
