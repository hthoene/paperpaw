//! Retained native owners behind the screen router's synchronous dispatch API.
use super::{
    Error, FlatHost, flat::FlatDepth, native_screen::Dispatch, registrations::DrawFrame,
    retained::Retained, shell,
};
use crate::abi7::core_frame::CoreFrame;
use paperpaw_graphics::Canvas;
use paperpaw_native::host_glue::{self, Services};
use paperpaw_sdk::Env;

pub(crate) struct NativeDispatch<S: Services + 'static, M: 'static> {
    instance: Retained<S, M>,
    depth: FlatDepth,
    host: FlatHost,
    report: fn(&S, u32, i32),
    identity: super::native_instances::NativeIdentity,
    events: super::event_tags::EventTags,
}

impl<S: Services + 'static, M: 'static> NativeDispatch<S, M> {
    /// An instance whose event tags are read from its registrations, which
    /// the tests that install nothing do.
    #[cfg(test)]
    pub(crate) fn new(
        instance: Retained<S, M>,
        depth: FlatDepth,
        host: FlatHost,
        report: fn(&S, u32, i32),
        identity: super::native_instances::NativeIdentity,
    ) -> Self {
        let events = event_tags(&instance, &depth);
        Self::with_events(instance, depth, host, report, identity, events)
    }

    /// An instance with the event tags its install admitted.
    pub(crate) fn with_events(
        instance: Retained<S, M>,
        depth: FlatDepth,
        host: FlatHost,
        report: fn(&S, u32, i32),
        identity: super::native_instances::NativeIdentity,
        events: super::event_tags::EventTags,
    ) -> Self {
        Self {
            instance,
            depth,
            host,
            report,
            identity,
            events,
        }
    }

    fn frame<'a>(
        &'a self,
        env: &'a Env<'a>,
        epoch: u64,
        instance: Option<paperpaw_sdk::ScreenKey>,
        directory: Option<&'a super::native_declarations::NativeDeclarations>,
    ) -> CoreFrame<'a> {
        CoreFrame {
            callback: crate::abi7::core_frame::CallbackStamp::new(self.identity.owner(), epoch)
                .ok(),
            instance: instance.map(paperpaw_sdk::ScreenKey::serial),
            env,
            meter: &self.host.meter,
            depth: Some(self.depth.as_depth()),
            directory,
            stack_generation: self.host.stack_rows.generation(),
            stack_rows: &self.host.stack_rows,
            font_removals: &self.host.font_removals,
            registry_generation: &self.host.registry_generation,
            usage: env.usage(),
            plugins: env.plugins(),
            work: env.work(),
            log: self.host.log,
            ui: self.identity.ui_services(),
        }
    }

    fn invoke(
        &self,
        token: u32,
        env: &Env<'_>,
        instance: Option<paperpaw_sdk::ScreenKey>,
        invocation: shell::TokenInvocation<'_>,
        stage: Option<&mut super::commands::CommandStage<'_>>,
    ) -> Result<i32, i32> {
        let directory = self.host.upgrade_directory()?;
        let epoch = self.identity.callback()?;
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        // A screen's callback is on the stack until this returns (`core.net`
        // asks whether the screen is showing).
        let _screen = instance.map(|key| {
            self.host
                .stack_rows
                .enter_callback(self.identity.owner(), key.serial())
        });
        let frame = self.frame(env, epoch.epoch(), instance, directory.as_deref());
        // What the callback froze of the stages it ran ends with it.
        let _release = super::stage_members::Release::new(
            directory.as_deref(),
            epoch.epoch(),
            self.depth.as_depth().current() == 0,
        );
        let result = match stage {
            Some(stage) => shell::dispatch_token_staged(
                batch,
                token,
                self.depth.as_depth(),
                &frame,
                invocation,
                stage,
            ),
            None => shell::dispatch_token(batch, token, self.depth.as_depth(), &frame, invocation),
        }
        .map_err(status);
        // Only 0 and 1 are answers: an error or any other status is a failure.
        if !matches!(result, Ok(0 | 1)) {
            epoch.set_failed();
        }
        result
    }
    /// A host service's event for the callback registered for its tag, with
    /// the command stage of that callback where the caller has one.
    fn service_call(
        &self,
        bytes: &[u8],
        env: &Env<'_>,
        stage: Option<&mut super::commands::CommandStage<'_>>,
    ) -> Result<i32, i32> {
        let tag = super::registrations::RawRecord::single(bytes)?.tag();
        let mut token = None;
        self.instance
            .registrations(self.depth.as_depth(), &mut |_, record| {
                if let super::Registration::Event {
                    event_tag,
                    token: at,
                } = record
                    && event_tag == tag
                    && token.is_none()
                {
                    token = Some(at);
                }
            })
            .map_err(|error| status(error.into()))?;
        // An answer goes to the plugin that asked, with no registration
        // (`ui.md` 3.2): the SDK takes addressed events by their tag.
        if tag == super::registrations::EVENT_ANSWERED_TAG {
            token.get_or_insert(0);
        }
        let token = token.ok_or(host_glue::ERR_UNSUPPORTED)?;
        self.invoke(
            token,
            env,
            None,
            shell::TokenInvocation::Event(bytes),
            stage,
        )
    }
    fn render(
        &self,
        token: u32,
        frame: &DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
        instance: Option<paperpaw_sdk::ScreenKey>,
    ) -> Result<i32, i32> {
        let directory = self.host.upgrade_directory()?;
        let epoch = self.identity.callback()?;
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        let _release = super::stage_members::Release::new(
            directory.as_deref(),
            epoch.epoch(),
            self.depth.as_depth().current() == 0,
        );
        let result = shell::dispatch_token_draw(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env, epoch.epoch(), instance, directory.as_deref()),
            frame,
            canvas,
        )
        .map_err(status);
        if matches!(result, Ok(0)) {
            self.identity.mark_drew();
        }
        result
    }
}

impl<S: Services + 'static, M: 'static> Dispatch for NativeDispatch<S, M> {
    fn take_open_info(&self) -> Option<super::ui_store::Info> {
        self.identity.take_open_info()
    }

    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, None, shell::TokenInvocation::Event(bytes), None)
    }

    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, None, shell::TokenInvocation::Input(bytes), None)
    }

    fn draw(
        &self,
        token: u32,
        frame: &DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        self.render(token, frame, env, canvas, None)
    }
    fn screen_callback(
        &self,
        instance: Option<paperpaw_sdk::ScreenKey>,
        token: u32,
        bytes: &[u8],
        env: &Env<'_>,
        input: bool,
    ) -> Result<i32, i32> {
        let invocation = if input {
            shell::TokenInvocation::Input(bytes)
        } else {
            shell::TokenInvocation::Event(bytes)
        };
        self.invoke(token, env, instance, invocation, None)
    }
    fn screen_draw(
        &self,
        instance: Option<paperpaw_sdk::ScreenKey>,
        token: u32,
        frame: &DrawFrame<'_>,
        env: &Env<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<i32, i32> {
        self.render(token, frame, env, canvas, instance)
    }

    fn stage(&self, token: u32, bytes: &[u8], out: &mut [u8], env: &Env<'_>) -> Result<i32, i32> {
        let directory = self.host.upgrade_directory()?;
        let epoch = self.identity.callback()?;
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        let _release = super::stage_members::Release::new(
            directory.as_deref(),
            epoch.epoch(),
            self.depth.as_depth().current() == 0,
        );
        shell::dispatch_stage(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env, epoch.epoch(), None, directory.as_deref()),
            bytes,
            out,
        )
        .map_err(status)
    }
    fn dropped(&self, token: u32, status: i32) {
        (self.report)(
            &self.instance.host().owner().context().services,
            token,
            status,
        );
    }
}

/// The event tags the committed registrations name, read once at the start.
/// A failed read or a tag beyond the set leaves the owner unsubscribed to it.
#[cfg(test)]
fn event_tags<S: Services + 'static, M: 'static>(
    instance: &Retained<S, M>,
    depth: &FlatDepth,
) -> super::event_tags::EventTags {
    let mut tags = super::event_tags::EventTags::default();
    let _ = instance.registrations(depth.as_depth(), &mut |_, record| {
        if let super::registrations::Registration::Event { event_tag, .. } = record {
            let _ = tags.insert(event_tag);
        }
    });
    tags
}

fn status(error: shell::Error) -> i32 {
    match error {
        shell::Error::Status(code) | shell::Error::Factory(code) => code,
        shell::Error::Allocation | shell::Error::Lifecycle(Error::Memory) => {
            host_glue::ERR_NO_MEMORY
        }
        shell::Error::Lifecycle(Error::Busy | Error::Depth) | shell::Error::Refused => {
            host_glue::ERR_BUSY
        }
        shell::Error::Lifecycle(Error::Capacity) => host_glue::ERR_NO_SPACE,
        shell::Error::Lifecycle(Error::State) | shell::Error::Attempt => host_glue::ERR_STATE,
        _ => host_glue::ERR_INVALID,
    }
}

impl<S: Services + 'static, M: 'static> super::native_instances::NativeInstance
    for NativeDispatch<S, M>
{
    fn running(&self) -> bool {
        self.instance.host().owner().is_live()
    }
    fn stop(&self) {
        self.instance.host().stop();
    }
    fn callback_depth(&self) -> u8 {
        self.depth.as_depth().0.current()
    }
    fn subscribed(&self, tag: u32) -> bool {
        self.events.contains(tag)
    }
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.instance.host().image());
    }
    fn service_event(&self, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.service_call(bytes, env, None)
    }
    fn service_event_staged(
        &self,
        bytes: &[u8],
        env: &Env<'_>,
        stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.service_call(bytes, env, Some(stage))
    }
    fn input_staged(
        &self,
        token: u32,
        bytes: &[u8],
        env: &Env<'_>,
        stage: &mut super::commands::CommandStage<'_>,
    ) -> Result<i32, i32> {
        self.invoke(
            token,
            env,
            None,
            shell::TokenInvocation::Input(bytes),
            Some(stage),
        )
    }
    fn retire(&self, owner: u64) -> Result<(), i32> {
        self.instance.host().stop();
        if owner != self.identity.owner() {
            return Err(host_glue::ERR_STATE);
        }
        self.instance
            .host()
            .owner()
            .prepare_teardown()
            .map_err(|error| status(shell::Error::Lifecycle(error)))
    }
}
