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
}

impl<S: Services + 'static, M: 'static> NativeDispatch<S, M> {
    pub(crate) fn new(
        instance: Retained<S, M>,
        depth: FlatDepth,
        host: FlatHost,
        report: fn(&S, u32, i32),
        identity: super::native_instances::NativeIdentity,
    ) -> Self {
        Self {
            instance,
            depth,
            host,
            report,
            identity,
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
            usage: env.usage(),
            plugins: env.plugins(),
            work: env.work(),
        }
    }

    fn invoke(
        &self,
        token: u32,
        env: &Env<'_>,
        instance: Option<paperpaw_sdk::ScreenKey>,
        invocation: shell::TokenInvocation<'_>,
    ) -> Result<i32, i32> {
        let directory = self.host.upgrade_directory()?;
        let epoch = self.identity.callback()?;
        let batch = self.instance.batch().map_err(|e| status(e.into()))?;
        shell::dispatch_token(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env, epoch.epoch(), instance, directory.as_deref()),
            invocation,
        )
        .map_err(status)
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
        shell::dispatch_token_draw(
            batch,
            token,
            self.depth.as_depth(),
            &self.frame(env, epoch.epoch(), instance, directory.as_deref()),
            frame,
            canvas,
        )
        .map_err(status)
    }
}

impl<S: Services + 'static, M: 'static> Dispatch for NativeDispatch<S, M> {
    fn event(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, None, shell::TokenInvocation::Event(bytes))
    }

    fn input(&self, token: u32, bytes: &[u8], env: &Env<'_>) -> Result<i32, i32> {
        self.invoke(token, env, None, shell::TokenInvocation::Input(bytes))
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
        self.invoke(token, env, instance, invocation)
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

fn status(error: shell::Error) -> i32 {
    match error {
        shell::Error::Status(code) | shell::Error::Factory(code) => code,
        shell::Error::Allocation | shell::Error::Lifecycle(Error::Memory) => {
            host_glue::ERR_NO_MEMORY
        }
        shell::Error::Lifecycle(Error::Busy | Error::Depth) => host_glue::ERR_BUSY,
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
    fn with_image(&self, visit: &mut dyn FnMut(Option<&[u8]>)) {
        visit(self.instance.host().image());
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
