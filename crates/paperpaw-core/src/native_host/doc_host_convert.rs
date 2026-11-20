//! The conversion hook's side of the bound host (`docs/abi/document.md` 4.1):
//! the subscribers of `core.convert` and the text they give for a foreign
//! element, asked at the level of the format's own calls.
use super::{Bound, FrameRoute, Target};
use super::{ERR_NO_MEMORY, ERR_STATE};
use crate::native_host::stages_wire::core_convert_v1 as hook;
use alloc::{string::String, vec::Vec};

/// A buffer of `length` zeros, fallibly.
fn zeroed(length: usize) -> Result<Vec<u8>, i32> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| ERR_NO_MEMORY)?;
    bytes.resize(length, 0);
    Ok(bytes)
}

impl Bound<'_> {
    /// The subscribers of `core.convert` in stage order, at most the stage's.
    pub(super) fn hooks(&self) -> Result<([Option<Target>; hook::MAX_SUBSCRIBERS], usize), i32> {
        let mut found = [None; hook::MAX_SUBSCRIBERS];
        let mut count = 0;
        let mut result = Err(ERR_STATE);
        self.frame.with_declarations(&mut |any| {
            let Some(store) =
                any.downcast_ref::<crate::native_host::declaration_store::DeclarationStore>()
            else {
                return;
            };
            result = store.stage_subscribers(hook::KEY).map(|mut all| {
                paperpaw_sort::sort_unstable_by_key(&mut all, |s| (s.order, s.owner, s.index));
                for subscriber in all.iter().take(hook::MAX_SUBSCRIBERS) {
                    found[count] = Some(Target {
                        plugin: subscriber.owner,
                        token: subscriber.token,
                    });
                    count += 1;
                }
            });
        })?;
        result?;
        Ok((found, count))
    }

    /// The text a hook gives for a foreign element (`document.md` 4.1): each
    /// subscriber in stage order is asked at the level of the format's own
    /// calls; the first answer with a text is it. A hook that fails, is not
    /// made yet (it is asked to start) or says nothing passes it on.
    #[inline(never)] // own frame: the stage's buffers stay out of the service's
    pub(super) fn convert_foreign(
        &self,
        element: &str,
        markup: &str,
    ) -> Result<Option<String>, i32> {
        let (hooks, count) = self.hooks()?;
        if count == 0 {
            return Ok(None);
        }
        let ask = hook::Record::ConvAsk(hook::ConvAsk::new(element, 0, markup));
        let mut payload = zeroed(hook::MAX_INPUT)?;
        let length = hook::input_encode(&[ask], &mut payload)?;
        let frame_route = FrameRoute(self.frame);
        let route = self.route.unwrap_or(&frame_route);
        let mut reply = zeroed(hook::MAX_REPLY)?;
        for target in hooks.iter().take(count).flatten() {
            let Ok(written) = route.convert(*target, &payload[..length], &mut reply) else {
                continue;
            };
            let text = reply
                .get(..written)
                .and_then(|bytes| hook::reply_decode(bytes).ok())
                .and_then(|mut rows| rows.next());
            let Some(Ok(hook::Record::ConvText(answer))) = text else {
                continue;
            };
            if answer.text.is_empty() {
                continue;
            }
            let mut text = String::new();
            text.try_reserve_exact(answer.text.len())
                .map_err(|_| ERR_NO_MEMORY)?;
            text.push_str(answer.text);
            return Ok(Some(text));
        }
        Ok(None)
    }
}
